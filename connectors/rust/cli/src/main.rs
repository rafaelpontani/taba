use std::io::{self, Write};

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut out = io::BufWriter::new(io::stdout().lock());
    let mut err = io::stderr().lock();
    let code = taba_cli::run(
        &taba_cli::system::RealPlatform,
        &args,
        &mut out,
        &mut err,
        &mut input,
    );
    let _ = out.flush();
    std::process::exit(code);
}
