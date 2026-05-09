#![allow(non_snake_case)]

use firecracker_sdk::env_value_or_default_int;

#[test]
fn TestEnvValueOrDefaultInt() {
    let key = "FIRECRACKER_SDK_TEST_ENV";
    unsafe {
        std::env::remove_var(key);
    }
    assert_eq!(42, env_value_or_default_int(key, 42));

    unsafe {
        std::env::set_var(key, "17");
    }
    assert_eq!(17, env_value_or_default_int(key, 42));

    unsafe {
        std::env::set_var(key, "0");
    }
    assert_eq!(42, env_value_or_default_int(key, 42));

    unsafe {
        std::env::remove_var(key);
    }
}
