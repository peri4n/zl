use std::fs;

use clap::{Parser, Subcommand};
use tera::{Context, Tera};

#[macro_use]
extern crate lazy_static;

lazy_static! {
    pub static ref TEMPLATES: Tera = {
        let tera = match Tera::new(".zl/templates/**/*") {
            Ok(t) => t,
            Err(e) => {
                println!("Parsing error(s): {}", e);
                ::std::process::exit(1);
            }
        };
        tera
    };
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug, Clone)]
enum Command {
    Validate {
        /// Path of the markdown file to validate
        #[arg(short, long)]
        path: String,
    },
    New {
        /// Path of the new markdown file
        #[arg(short, long)]
        path: String,

        /// Template of the new markdown file
        #[arg(short, long)]
        template: String,
    },
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();

    match &cli.cmd {
        Command::Validate { path } => {
            let input = fs::read_to_string(path).map_err(|_| "File not found")?;
            let _ = markdown::to_mdast(&input, &markdown::ParseOptions::default());
        },
        Command::New { path, template } => {
            let mut context = Context::new();
            context.insert("title", "test");
            let expr = TEMPLATES.render(&format!("{}.md", template), &context)
                .map_err(|e| e.to_string())?;

            fs::write(path, expr)
                .map_err(|e| format!("Can not write file: {}", e.to_string()))?
        },
    }

    Ok(())

}
