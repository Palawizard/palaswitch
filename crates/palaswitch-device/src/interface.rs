use std::ffi::CString;

pub const LOGITECH_VENDOR_ID: u16 = 0x046D;
const HIDPP_USAGE_PAGE: u16 = 0xFF00;
const SHORT_USAGE: u16 = 0x0001;
const LONG_USAGE: u16 = 0x0002;

/// One HID top-level collection, as listed by the operating system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub path: CString,
    pub product: String,
}

/// The HID++ collections of one USB interface: the long one is required,
/// the short one carries the receiver's HID++ 1.0 errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub product_id: u16,
    pub product: String,
    pub long_path: CString,
    pub short_path: Option<CString>,
}

/// Windows lists each collection with its own path, the same up to its
/// `&ColNN` part: `\\?\HID#VID_046D&PID_C547&MI_02&Col02#...`.
fn interface_key(path: &CString) -> String {
    let path = path.to_string_lossy().to_ascii_lowercase();
    match path.find("&col") {
        Some(start) => path[..start].to_owned(),
        None => path,
    }
}

/// Pairs the short and long HID++ collections of each interface. Interfaces
/// without a long collection cannot speak HID++ 2.0 and are left out.
pub fn group_hidpp_collections(collections: &[Collection]) -> Vec<Interface> {
    let hidpp = collections
        .iter()
        .filter(|collection| collection.usage_page == HIDPP_USAGE_PAGE);
    let mut interfaces: Vec<Interface> = hidpp
        .clone()
        .filter(|collection| collection.usage == LONG_USAGE)
        .map(|long| Interface {
            product_id: long.product_id,
            product: long.product.clone(),
            long_path: long.path.clone(),
            short_path: None,
        })
        .collect();
    for short in hidpp.filter(|collection| collection.usage == SHORT_USAGE) {
        let key = interface_key(&short.path);
        if let Some(interface) = interfaces
            .iter_mut()
            .find(|interface| interface_key(&interface.long_path) == key)
        {
            interface.short_path = Some(short.path.clone());
        }
    }
    interfaces
}

#[cfg(test)]
mod tests {
    use super::*;

    // Paths of the LIGHTSPEED receiver of the G502 X PLUS, listed by hidapi on 2026-11-20.
    const SHORT_PATH: &str = r"\\?\HID#VID_046D&PID_C547&MI_02&Col01#a&26685ccb&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
    const LONG_PATH: &str = r"\\?\HID#VID_046D&PID_C547&MI_02&Col02#a&26685ccb&0&0001#{4d1e55b2-f16f-11cf-88cb-001111000030}";

    fn collection(usage: u16, path: &str) -> Collection {
        Collection {
            product_id: 0xC547,
            usage_page: HIDPP_USAGE_PAGE,
            usage,
            path: CString::new(path).expect("path"),
            product: "USB Receiver".to_owned(),
        }
    }

    #[test]
    fn short_and_long_collections_of_the_receiver_form_one_interface() {
        let collections = [
            collection(SHORT_USAGE, SHORT_PATH),
            collection(LONG_USAGE, LONG_PATH),
        ];

        let interfaces = group_hidpp_collections(&collections);

        assert_eq!(
            interfaces
                .iter()
                .map(|interface| (
                    interface.long_path.to_str().ok(),
                    interface.short_path.is_some()
                ))
                .collect::<Vec<_>>(),
            vec![(Some(LONG_PATH), true)]
        );
    }

    // Paths of the same mouse plugged in with its cable, listed by hidapi on 2026-11-20.
    const WIRED_SHORT_PATH: &str = r"\\?\HID#VID_046D&PID_C095&MI_02&Col01#8&e6c451&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
    const WIRED_LONG_PATH: &str = r"\\?\HID#VID_046D&PID_C095&MI_02&Col02#8&e6c451&0&0001#{4d1e55b2-f16f-11cf-88cb-001111000030}";

    #[test]
    fn receiver_and_cable_give_two_interfaces_each_with_its_own_short_collection() {
        let wired = |usage, path: &str| Collection {
            product_id: 0xC095,
            product: "G502 X PLUS".to_owned(),
            ..collection(usage, path)
        };
        let collections = [
            collection(SHORT_USAGE, SHORT_PATH),
            wired(LONG_USAGE, WIRED_LONG_PATH),
            collection(LONG_USAGE, LONG_PATH),
            wired(SHORT_USAGE, WIRED_SHORT_PATH),
        ];

        let interfaces = group_hidpp_collections(&collections);

        assert_eq!(
            interfaces
                .iter()
                .map(|interface| (
                    interface.product_id,
                    interface
                        .short_path
                        .as_ref()
                        .and_then(|path| path.to_str().ok())
                ))
                .collect::<Vec<_>>(),
            vec![(0xC095, Some(WIRED_SHORT_PATH)), (0xC547, Some(SHORT_PATH))]
        );
    }

    #[test]
    fn short_collection_alone_is_not_an_interface() {
        let interfaces = group_hidpp_collections(&[collection(SHORT_USAGE, SHORT_PATH)]);

        assert!(interfaces.is_empty());
    }
}
