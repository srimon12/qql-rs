#[derive(serde::Serialize)]
pub struct ExecResponse {
    pub ok: bool,
    pub operation: String,
    pub message: String,
}

#[derive(serde::Serialize)]
pub struct ExplainResponse {
    pub ok: bool,
    pub query: String,
    pub plan: String,
}

#[derive(serde::Serialize)]
pub struct ScriptResponse {
    pub ok: bool,
    pub command: String,
    pub path: String,
    pub succeeded: u32,
    pub failed: u32,
    pub message: String,
}

#[derive(serde::Serialize)]
pub struct VersionResponse {
    pub ok: bool,
    pub command: String,
    pub version: String,
    pub message: String,
}

pub fn print_success(msg: &str) {
    println!("\x1b[32m\u{2713}\x1b[0m {}", msg);
}

pub fn print_error(msg: &str) {
    eprintln!("\x1b[31m\u{2717}\x1b[0m {}", msg);
}

pub fn print_banner() {
    println!(
        "\n\x1b[36m\u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2557}\x1b[0m\n\x1b[36m\u{2551}\x1b[0m  \x1b[1;36mQQL \u{2014} Qdrant Query Language\x1b[0m           \x1b[36m\u{2551}\x1b[0m\n\x1b[36m\u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d}\x1b[0m\n",
    );
}
