pub const NAME: &str = "KASPilot";

pub const BANNER: &str = r#"
 _  __    _    ____  ____  _ _       _
| |/ /   / \  / ___||  _ \(_) | ___ | |_
| ' /   / _ \ \___ \| |_) | | |/ _ \| __|
| . \  / ___ \ ___) |  __/| | | (_) | |_
|_|\_\/_/   \_\____/|_|   |_|_|\___/ \__|
"#;

pub fn print_banner(mode: &str) {
    println!("{BANNER}");
    println!("{NAME} :: {mode}");
    println!("Kaspa ASIC fleet controller / GPU supervisor / CPU dev miner\n");
}
