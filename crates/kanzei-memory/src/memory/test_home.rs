//! Global memory tests run with a process-owned home and a completion receipt.
use std::path::Path;

pub(super) fn run(test_name: &str, test: impl FnOnce(&Path)) {
    let home = kanzei_base::test_support::TestDir::new("kz-memory-home-");
    kanzei_base::test_support::in_subprocess(
        test_name,
        &[("KANZEI_HOME", home.as_os_str())],
        || {
            let isolated_home = std::env::var_os("KANZEI_HOME").expect("isolated memory home");
            test(Path::new(&isolated_home));
        },
    );
}
