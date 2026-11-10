//! Prints whether Conch can use Accessibility and why the focused control is
//! or is not an eligible paste target. Focus the target app during the delay.
//! Usage: focus_diagnostic [delay seconds]
use conch_lib::{
    delivery::Desktop,
    platform::{NativeDesktop, permission},
};
fn main() {
    let delay: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    println!("accessibility_granted={}", permission());
    std::thread::sleep(std::time::Duration::from_secs(delay));
    let mut desktop = NativeDesktop::default();
    let eligible = desktop.focus().is_some();
    println!("eligible_focus={eligible}");
    if !eligible {
        println!("reason={}", desktop.fallback_reason());
    }
}
