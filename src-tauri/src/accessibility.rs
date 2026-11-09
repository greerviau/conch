pub fn repair_with(
    reset: impl FnOnce(&str, &[&str]) -> Result<(), String>,
    request: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    reset(
        "/usr/bin/tccutil",
        &["reset", "Accessibility", "app.conch.desktop"],
    )?;
    request()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repairs_only_conch_accessibility_then_requests_current_process_access() {
        let order = std::cell::RefCell::new(vec![]);
        repair_with(
            |program, args| {
                assert_eq!(program, "/usr/bin/tccutil");
                assert_eq!(args, ["reset", "Accessibility", "app.conch.desktop"]);
                order.borrow_mut().push("reset");
                Ok(())
            },
            || {
                order.borrow_mut().push("request");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*order.borrow(), vec!["reset", "request"]);
    }
    #[test]
    fn failed_reset_does_not_request_permission() {
        let requested = std::cell::Cell::new(false);
        assert_eq!(
            repair_with(
                |_, _| Err("Reset failed".into()),
                || {
                    requested.set(true);
                    Ok(())
                }
            )
            .unwrap_err(),
            "Reset failed"
        );
        assert!(!requested.get());
    }
}
