use std::{env, fs, io};

fn main() {
    println!("Hello, world!!!");
    let mut args_iter = env::args();
    args_iter.next(); // ignore path

    let command = args_iter.next().unwrap_or_default();

    let result = match command.as_str() {
        "" => Err("Error: no command specified".to_string()),
        "init" => match init_repo() {
            Err(error) => Err(format!("Failed to initalize repository: IO Error: {error}")),
            Ok(_) => Ok(()),
        },
        _ => Err(format!("Error: unknown command '{}'", command)),
    };

    match result {
        Err(error) => {
            println!("Fatal: {}", error);
        }
        Ok(_) => (),
    }
}

fn init_repo() -> io::Result<()> {
    fs::create_dir(".jbackup")?;
    fs::write(".jbackup/branches", "main\tNULL")?;
    fs::write(".jbackup/head", "NULL")?;
    Ok(())
}
