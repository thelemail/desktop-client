use std::process::ExitCode;

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [config, archive, signature] = args.as_slice() else {
        return Err("usage: verify-update <tauri.conf.json> <archive> <archive.sig>".to_owned());
    };
    let read = |path: &str| std::fs::read(path).map_err(|e| format!("{path}: {e}"));
    let config = String::from_utf8(read(config)?).map_err(|_| "the config is not text")?;
    let signature = String::from_utf8(read(signature)?).map_err(|_| "the signature is not text")?;
    let pubkey = thelemail_release::pubkey_from_config(&config)?;
    thelemail_release::verify(&pubkey, &read(archive)?, &signature)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => {
            println!("verify-update: signature matches the pinned key");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("verify-update: {err}");
            ExitCode::FAILURE
        }
    }
}
