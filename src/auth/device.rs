pub fn detect_device(user_agent: &str) -> String {
    let ua = user_agent.to_lowercase();

    if ua.contains("iphone") {
        "iPhone".to_string()
    } else if ua.contains("ipad") {
        "iPad".to_string()
    } else if ua.contains("android") {
        "Android".to_string()
    } else if ua.contains("windows") {
        "Windows".to_string()
    } else if ua.contains("macintosh") || ua.contains("mac os") {
        "macOS".to_string()
    } else if ua.contains("linux") {
        "Linux".to_string()
    } else {
        "Unknown".to_string()
    }
}
