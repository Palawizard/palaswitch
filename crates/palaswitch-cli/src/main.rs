//! Command line tool to try the HID++ layer on a connected mouse.

mod plan;

use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use palaswitch_device::{Connection, HidApi, HidTransport, Mouse, find_mice, hidpp_interfaces};
use palaswitch_hidpp::features::{Mode, Profile};
use palaswitch_hidpp::{Error, Transport};
use plan::{
    Choice, MAX_SNIFF_SECS, MAX_STRESS_COUNT, choose_profile, describe_profile,
    mouse_may_be_asleep, parse_bounded, stress_cycle,
};

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
        ["list"] => list(&api, options.product_id),
        ["get"] => {
            let mut mouse = first_mouse(&api, options.product_id)?;
            let sector = mouse.current_profile().map_err(|error| error.to_string())?;
            println!("{}", describe_profile(sector));
            Ok(())
        }
        ["set", number] => set(&api, options.product_id, parse_number(number)?),
        ["sniff", secs] => sniff(
            &api,
            options.product_id,
            parse_bounded(secs, MAX_SNIFF_SECS)?,
        ),
        ["stress", count] => stress(
            &api,
            options.product_id,
            parse_bounded(count, MAX_STRESS_COUNT)?,
        ),
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
    let asleep = mouse_may_be_asleep(&skipped, product_id);
    mice.into_iter()
        .find(|mouse| product_id.is_none_or(|pid| mouse.product_id == pid))
        .ok_or_else(|| no_mouse_message(asleep))
}

fn no_mouse_message(asleep: bool) -> String {
    if asleep {
        "no mouse answered: move it or switch it on, then try again".to_owned()
    } else {
        "no mouse with onboard profiles found".to_owned()
    }
}

fn describe_connection(connection: Connection) -> String {
    match connection {
        Connection::Wired => "wired".to_owned(),
        Connection::Receiver(index) => format!("receiver, device index {index}"),
    }
}

fn list(api: &HidApi, product_id: Option<u16>) -> Result<(), String> {
    let (mice, skipped) = find_mice(api);
    let selected = |pid: u16| product_id.is_none_or(|wanted| wanted == pid);
    for skip in skipped.iter().filter(|skip| selected(skip.product_id)) {
        println!("{:04X}: skipped ({})", skip.product_id, skip.reason);
    }
    let mice: Vec<Mouse> = mice
        .into_iter()
        .filter(|mouse| selected(mouse.product_id))
        .collect();
    if mice.is_empty() {
        return Err(no_mouse_message(mouse_may_be_asleep(&skipped, product_id)));
    }
    for mut mouse in mice {
        let info = mouse.info();
        println!(
            "{} ({:04X}, {}): {} profiles, {} in ROM",
            mouse.name,
            mouse.product_id,
            describe_connection(mouse.connection),
            info.profile_count,
            info.rom_profile_count
        );
        if let Err(error) = print_profiles(&mut mouse) {
            println!("  could not read the profiles: {error}");
        }
    }
    Ok(())
}

fn print_profiles(mouse: &mut Mouse) -> Result<(), Error> {
    let mode = mouse.mode()?;
    let current = mouse.current_profile()?;
    println!("  mode {mode:?}");
    for profile in mouse.profiles()? {
        println!(
            "  profile {}{}{}{}",
            profile.number(),
            if profile.is_rom() { " (factory)" } else { "" },
            if profile.enabled { "" } else { " (disabled)" },
            if profile.sector == current {
                " (active)"
            } else {
                ""
            }
        );
    }
    Ok(())
}

fn set(api: &HidApi, product_id: Option<u16>, number: u16) -> Result<(), String> {
    let mut mouse = first_mouse(api, product_id)?;
    let profiles = mouse.profiles().map_err(|error| error.to_string())?;
    let profile = choose_profile(&profiles, number).map_err(|choice| match choice {
        Choice::Missing => format!("{} has no profile {number}", mouse.name),
        Choice::Disabled => format!("profile {number} is disabled on {}", mouse.name),
    })?;
    let start = Instant::now();
    mouse
        .switch_to(profile.sector)
        .map_err(|error| error.to_string())?;
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

const RESTORE_ATTEMPTS: usize = 3;
const RESTORE_PAUSE: Duration = Duration::from_millis(500);

fn stress(api: &HidApi, product_id: Option<u16>, count: usize) -> Result<(), String> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&interrupted);
    ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst))
        .map_err(|error| format!("could not catch Ctrl+C: {error}"))?;

    let mut mouse = first_mouse(api, product_id)?;
    let profiles = mouse.profiles().map_err(|error| error.to_string())?;
    let mode = mouse.mode().map_err(|error| error.to_string())?;
    if mode != Mode::Onboard {
        println!(
            "note: the mouse was in mode {mode:?}; switching profiles puts it in onboard mode"
        );
    }
    let initial = mouse.current_profile().map_err(|error| error.to_string())?;
    let cycle: Vec<Profile> = stress_cycle(&profiles, initial);
    if cycle.len() < 2 {
        return Err("at least two enabled profiles are needed".to_owned());
    }

    let mut durations = Vec::with_capacity(count);
    let mut failures = Vec::new();
    let mut transport_lost = false;
    for round in 0..count {
        if interrupted.load(Ordering::SeqCst) {
            println!("interrupted after {round} switches");
            break;
        }
        let target = cycle[round % cycle.len()];
        let start = Instant::now();
        match mouse.switch_to(target.sector) {
            Ok(()) => durations.push(start.elapsed()),
            Err(error @ Error::Transport(_)) => {
                failures.push(format!("switch {}: {error}", round + 1));
                transport_lost = true;
                break;
            }
            Err(error) => failures.push(format!(
                "switch {} to profile {}: {error}",
                round + 1,
                target.number()
            )),
        }
    }

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
    if initial == 0 {
        println!("no profile was active at the start, nothing to restore");
    } else if transport_lost {
        let connection = mouse.connection;
        let product_id = mouse.product_id;
        drop(mouse);
        // The device list of `api` was read at start; the mouse may come back
        // under a new path, so look again with a fresh one.
        let fresh = HidApi::new().map_err(|error| error.to_string())?;
        let mut found = find_mice(&fresh)
            .0
            .into_iter()
            .find(|mouse| mouse.product_id == product_id && mouse.connection == connection)
            .ok_or_else(|| {
                format!(
                    "the mouse went away; could not restore {}",
                    describe_profile(initial)
                )
            })?;
        restore(&mut found, initial)?;
    } else {
        restore(&mut mouse, initial)?;
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} switches failed", failures.len()))
    }
}

fn restore(mouse: &mut Mouse, sector: u16) -> Result<(), String> {
    let mut last_error = None;
    for attempt in 0..RESTORE_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(RESTORE_PAUSE);
        }
        match mouse.switch_to(sector) {
            Ok(()) => {
                println!("restored {}", describe_profile(sector));
                return Ok(());
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(format!(
        "could not restore {}: {}",
        describe_profile(sector),
        last_error
            .map(|error| error.to_string())
            .unwrap_or_default()
    ))
}
