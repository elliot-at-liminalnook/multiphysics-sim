use clap::{Parser, ValueEnum};

mod cad_app;
mod phenomena_app;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum Scene {
    #[default]
    Phenomena,
    Cad,
}

#[derive(Parser)]
#[command(about = "View models running on the generic multiphysics runtime")]
struct Args {
    #[arg(long, value_enum, default_value = "phenomena")]
    scene: Scene,
    /// CAD exchange file, required for the CAD scene.
    #[arg(long)]
    model: Option<String>,
    /// Phenomenon number or title fragment.
    #[arg(long)]
    exhibit: Option<String>,
}

fn main() {
    let args = Args::parse();
    match args.scene {
        Scene::Cad => {
            let Some(model) = args.model else {
                eprintln!("the CAD scene requires --model <file.simrobot.json>");
                std::process::exit(2);
            };
            cad_app::run(model);
        }
        Scene::Phenomena => {
            if let Some(exhibit) = args.exhibit {
                // SAFETY: single-threaded startup, before the viewer starts threads.
                unsafe { std::env::set_var("PHENOMENA_EXHIBIT", exhibit) };
            }
            phenomena_app::run();
        }
    }
}
