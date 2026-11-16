//! `disable_core_dumps` in its own process: the limit is process-wide and
//! cannot be raised back.

use rustix::process::{Resource, Rlimit, getrlimit};

#[test]
fn core_dumps_are_disabled_for_good() {
    ipsec_client_core::secrets::disable_core_dumps().expect("setrlimit");
    assert_eq!(
        getrlimit(Resource::Core),
        Rlimit {
            current: Some(0),
            maximum: Some(0),
        }
    );
    // Calling it again is harmless.
    ipsec_client_core::secrets::disable_core_dumps().expect("setrlimit, second call");
}
