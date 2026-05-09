#![allow(non_snake_case)]

use firecracker_sdk::{KernelArgs, parse_kernel_args};
use pretty_assertions::assert_eq;

#[test]
fn TestKernelArgsSerder() {
    let foo_val = "bar";
    let boo_val = "far";
    let doo_val = "a=silly=val";
    let empty_val = "";

    let args_string = format!(
        "foo={foo_val} blah doo={doo_val} huh={empty_val} bleh duh={empty_val} boo={boo_val}"
    );

    let expected = KernelArgs::from([
        ("foo".to_string(), Some(foo_val.to_string())),
        ("blah".to_string(), None),
        ("doo".to_string(), Some(doo_val.to_string())),
        ("huh".to_string(), Some(empty_val.to_string())),
        ("bleh".to_string(), None),
        ("duh".to_string(), Some(empty_val.to_string())),
        ("boo".to_string(), Some(boo_val.to_string())),
    ]);

    let actual = parse_kernel_args(args_string);
    assert_eq!(expected, actual);

    let reparsed = parse_kernel_args(actual.to_string());
    assert_eq!(expected, reparsed);
}
