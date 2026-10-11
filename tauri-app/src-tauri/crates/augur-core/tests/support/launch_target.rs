use std::time::Duration;

fn main() {
    let arguments = std::env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if let Some(record) = std::env::var_os("AUGUR_TEST_GUI_RECORD") {
        let payload = serde_json::json!({
            "pid": std::process::id(),
            "arguments": arguments,
        });
        std::fs::write(record, serde_json::to_vec(&payload).unwrap()).unwrap();
    }
    if let Ok(delay) = std::env::var("AUGUR_TEST_GUI_DELAY_MS") {
        if let Ok(milliseconds) = delay.parse::<u64>() {
            std::thread::sleep(Duration::from_millis(milliseconds));
        }
    }
}
