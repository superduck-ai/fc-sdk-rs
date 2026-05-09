#![allow(non_snake_case)]

use std::io::Cursor;

use firecracker_sdk::find_first_vendor_id;

#[test]
fn TestFindFirstVendorID() {
    let cases = [
        ("vendor_id : GenuineIntel", Some("GenuineIntel")),
        ("vendor_id : AuthenticAMD", Some("AuthenticAMD")),
        ("", None),
    ];

    for (input, expected) in cases {
        let vendor_id = find_first_vendor_id(Cursor::new(input)).unwrap();
        assert_eq!(expected.map(str::to_string), vendor_id);
    }
}
