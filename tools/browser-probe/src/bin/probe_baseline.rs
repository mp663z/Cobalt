fn main() {
    if browser_probe::input().is_none() {
        browser_probe::run_baseline();
    }
}
