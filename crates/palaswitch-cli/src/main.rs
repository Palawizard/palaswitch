//! Command line tool to try the HID++ layer on a connected mouse.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use palaswitch_device::{Connection, HidApi, HidTransport, Mouse, find_mice, hidpp_interfaces};
use palaswitch_hidpp::Transport;
use palaswitch_hidpp::features::Profile;

const USAGE: &str = "usage: palaswitch-cli [-v] [--pid <hex>] <command>

commands:
  list            show every mouse with onboard profiles
  get             show the active profile
  set <n>         make profile <n> active
  sniff <secs>    print every HID++ report for <secs> seconds
  stress <count>  switch between enabled profiles <count> times, then restore";

struct Options {
    verbose: bool,
    product_id: Option<u16>,
    command: Vec<String>,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options {
        verbose: false,
        product_id: None,
        command: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-v" => options.verbose = true,
            "--pid" => {
                let value = args.next().ok_or("--pid needs a value")?;
                let value = value.trim_start_matches("0x");
                let pid = u16::from_str_radix(value, 16)
                    .map_err(|_| format!("bad product ID {value}"))?;
                options.product_id = Some(pid);
            }
            _ => options.command.push(arg),
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    let options = match parse_options() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(if options.verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::WARN
        })
        .init();
    match run(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: &Options) -> Result<(), String> {
    let api = HidApi::new().map_err(|error| error.to_string())?;
    let command: Vec<&str> = options.command.iter().map(String::as_str).collect();
    match command.as_slice() {
        ["list"] => list(&api),
        ["get"] => {
            let mut mouse = first_mouse(&api, options.product_id)?;
            let sector = mouse.current_profile().map_err(|error| error.to_string())?;
            println!("profile {}", sector & 0xFF);
            Ok(())
        }
        ["set", number] => set(&api, options.product_id, parse_number(number)?),
        ["sniff", secs] => sniff(&api, options.product_id, parse_number(secs)?),
        ["stress", count] => stress(&api, options.product_id, parse_number(count)?),
        _ => Err(USAGE.to_owned()),
    }
}

fn parse_number<T: std::str::FromStr>(text: &str) -> Result<T, String> {
    text.parse().map_err(|_| format!("not a number: {text}"))
}

fn first_mouse(api: &HidApi, product_id: Option<u16>) -> Result<Mouse, String> {
    let (mice, skipped) = find_mice(api);
    for skip in &skipped {
        tracing::debug!(product_id = skip.product_id, reason = %skip.reason, "skipped interface");
    }
    mice.into_iter()
        .find(|mouse| product_id.is_none_or(|pid| mouse.product_id == pid))
        .ok_or_else(|| "no mouse with onboard profiles found".to_owned())
}

fn describe_connection(connection: Connection) -> String {
    match connection {
        Connection::Wired => "wired".to_owned(),
        Connection::Receiver(index) => format!("receiver, device index {index}"),
    }
}

fn list(api: &HidApi) -> Result<(), String> {
    let (mice, skipped) = find_mice(api);
    for skip in skipped {
        println!("{:04X}: skipped ({})", skip.product_id, skip.reason);
    }
    if mice.is_empty() {
        return Err("no mouse with onboard profiles found".to_owned());
    }
    for mut mouse in mice {
        let info = mouse.info();
        let mode = mouse.mode().map_err(|error| error.to_string())?;
        let current = mouse.current_profile().map_err(|error| error.to_string())?;
        let profiles = mouse.profiles().map_err(|error| error.to_string())?;
        println!(
            "{} ({:04X}, {}): {} profiles, {} in ROM, mode {mode:?}",
            mouse.name,
            mouse.product_id,
            describe_connection(mouse.connection),
            info.profile_count,
            info.rom_profile_count
        );
        for profile in profiles {
            println!(
                "  profile {}{}{}",
                profile.number(),
                if profile.enabled { "" } else { " (disabled)" },
                if profile.sector == current {
                    " (active)"
                } else {
                    ""
                }
            );
        }
    }
    Ok(())
}

fn set(api: &HidApi, product_id: Option<u16>, number: u16) -> Result<(), String> {
    let mut mouse = first_mouse(api, product_id)?;
    let start = Instant::now();
    mouse.switch_to(number).map_err(|error| error.to_string())?;
    println!(
        "profile {number} active ({} ms)",
        start.elapsed().as_millis()
    );
    Ok(())
}

fn sniff(api: &HidApi, product_id: Option<u16>, secs: u64) -> Result<(), String> {
    let interface = hidpp_interfaces(api)
        .into_iter()
        .find(|interface| product_id.is_none_or(|pid| interface.product_id == pid))
        .ok_or("no HID++ interface found")?;
    let mut transport = HidTransport::open(api, &interface).map_err(|error| error.to_string())?;
    println!(
        "# {:04X} {}, {secs} s",
        interface.product_id, interface.product
    );
    let start = Instant::now();
    let end = start + Duration::from_secs(secs);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if let Some(report) = transport.read(left).map_err(|error| error.to_string())? {
            let hex: Vec<String> = report.iter().map(|byte| format!("{byte:02X}")).collect();
            println!("{:>7} ms  {}", start.elapsed().as_millis(), hex.join(" "));
        }
    }
    Ok(())
}

fn stress(api: &HidApi, product_id: Option<u16>, count: usize) -> Result<(), String> {
    let mut mouse = first_mouse(api, product_id)?;
    let enabled: Vec<Profile> = mouse
        .profiles()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|profile| profile.enabled)
        .collect();
    if enabled.len() < 2 {
        return Err("at least two enabled profiles are needed".to_owned());
    }
    let initial = mouse.current_profile().map_err(|error| error.to_string())?;
    let mut durations = Vec::with_capacity(count);
    let mut failures = Vec::new();
    for round in 0..count {
        let target = enabled[round % enabled.len()].sector;
        let start = Instant::now();
        match mouse.switch_to(target) {
            Ok(()) => durations.push(start.elapsed()),
            Err(error) => failures.push(format!(
                "switch {} to profile {}: {error}",
                round + 1,
                target & 0xFF
            )),
        }
    }
    let restored = mouse.switch_to(initial);
    durations.sort();
    let median = durations
        .get(durations.len() / 2)
        .copied()
        .unwrap_or_default();
    let max = durations.last().copied().unwrap_or_default();
    println!(
        "{} {}: {}/{count} switches succeeded, median {} ms, max {} ms",
        mouse.name,
        describe_connection(mouse.connection),
        durations.len(),
        median.as_millis(),
        max.as_millis()
    );
    for failure in &failures {
        println!("  {failure}");
    }
    restored.map_err(|error| format!("could not restore profile {}: {error}", initial & 0xFF))?;
    println!("restored profile {}", initial & 0xFF);
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} switches failed", failures.len()))
    }
}
