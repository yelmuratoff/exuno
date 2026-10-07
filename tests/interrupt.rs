//! A raised signal sets the flag of every armed `Interrupt` in the process, so
//! a unit test raising one would make a parallel `sync` or `rollback` test
//! re-raise it and kill the test binary. This file is its own process.

#![cfg(unix)]

use exuno::transaction::interrupt::{Interrupt, status};
use signal_hook::consts::signal;

#[test]
fn a_trapped_signal_is_recorded_instead_of_ending_the_process() {
    let interrupt = Interrupt::arm();
    assert_eq!(interrupt.received(), None);
    signal_hook::low_level::raise(signal::SIGHUP).unwrap();
    assert_eq!(interrupt.received(), Some(signal::SIGHUP));
    assert_eq!(status(signal::SIGHUP), 129);
    assert_eq!(status(signal::SIGINT), 130);
}
