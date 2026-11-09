use conch_lib::{delivery::Desktop,platform::{NativeDesktop,permission}};
fn main() {
    println!("accessibility_granted={}",permission());
    println!("eligible_focus={}",NativeDesktop.focus().is_some());
}
