use std::fmt;
use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use serde_json::Value;
use tokio::{process::Command, task};
use which::which;

use crate::PusherError;

/// CLI arguments for the `save` subcommand.
#[derive(Debug, Args)]
pub struct SaveArgs {
    /// Container images to export. If omitted, an interactive selector is shown.
    #[arg(value_name = "IMAGE", num_args = 0..)]
    pub images: Vec<String>,

    /// Directory where tar archives will be written (defaults to current directory).
    #[arg(long, value_name = "DIR")]
    pub output_dir: Option<PathBuf>,

    /// Force use of a specific runtime binary instead of auto-detecting.
    #[arg(long, value_enum, value_name = "RUNTIME")]
    pub runtime: Option<RuntimeKind>,

    /// Overwrite existing tar archives without prompting.
    #[arg(long)]
    pub force: bool,
}

/// Supported container runtimes that can export images to tar archives.
#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum RuntimeKind {
    Docker,
    Nerdctl,
    Podman,
}

impl RuntimeKind {
    fn binary_name(&self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Nerdctl => "nerdctl",
            Self::Podman => "podman",
        }
    }

    fn display(&self) -> &'static str {
        match self {
            Self::Docker => "Docker",
            Self::Nerdctl => "nerdctl",
            Self::Podman => "Podman",
        }
    }
}

impl fmt::Display for RuntimeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display())
    }
}

/// Describes a resolved runtime binary along with convenience helpers.
struct ContainerRuntime {
    kind: RuntimeKind,
    binary: PathBuf,
}

impl ContainerRuntime {
    /// Attempts to find a usable runtime binary, honoring user preference when provided.
    fn detect(preferred: Option<RuntimeKind>) -> Result<Self, PusherError> {
        let mut order = match preferred {
            Some(kind) => vec![kind],
            None => vec![
                RuntimeKind::Docker,
                RuntimeKind::Nerdctl,
                RuntimeKind::Podman,
            ],
        };

        if preferred.is_some() {
            for fallback in [
                RuntimeKind::Docker,
                RuntimeKind::Nerdctl,
                RuntimeKind::Podman,
            ] {
                if !order.contains(&fallback) {
                    order.push(fallback);
                }
            }
        }

        for kind in order {
            if let Ok(path) = which(kind.binary_name()) {
                return Ok(Self { kind, binary: path });
            }
        }

        Err(PusherError::push_error(
            "No supported container runtime found. Install docker, nerdctl, or podman.",
        ))
    }

    fn label(&self) -> &'static str {
        self.kind.display()
    }

    /// Lists local images by shelling out to the selected runtime and parsing JSON rows.
    async fn list_images(&self) -> Result<Vec<LocalImage>, PusherError> {
        let mut cmd = Command::new(&self.binary);
        match self.kind {
            RuntimeKind::Docker | RuntimeKind::Podman => {
                cmd.args(["image", "ls", "--format", "{{json .}}"]);
            }
            RuntimeKind::Nerdctl => {
                cmd.args(["images", "--format", "{{json .}}"]);
            }
        }

        let output = cmd.output().await.map_err(|err| {
            PusherError::push_error(format!(
                "Failed to execute {} images command: {}",
                self.label(),
                err
            ))
        })?;

        if !output.status.success() {
            return Err(PusherError::push_error(format!(
                "{} images command failed: {}",
                self.label(),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut images = Vec::new();
        for line in stdout.lines() {
            let entry = line.trim();
            if entry.is_empty() {
                continue;
            }

            if entry.starts_with('[') {
                match serde_json::from_str::<Vec<Value>>(entry) {
                    Ok(list) => {
                        for value in list {
                            if let Some(image) = LocalImage::from_value(value) {
                                images.push(image);
                            }
                        }
                    }
                    Err(err) => println!("⚠️  Unable to parse images array: {}", err),
                }
                continue;
            }

            match serde_json::from_str::<Value>(entry) {
                Ok(value) => {
                    if let Some(image) = LocalImage::from_value(value) {
                        images.push(image);
                    }
                }
                Err(err) => println!("⚠️  Unable to parse image entry: {}", err),
            }
        }

        images.sort_by(|a, b| a.reference.cmp(&b.reference));
        Ok(images)
    }

    /// Invokes `<runtime> save` to write a tar archive to disk.
    async fn save_image(&self, reference: &str, output_path: &Path) -> Result<(), PusherError> {
        println!(
            "🧊 Saving {} via {} -> {}",
            reference,
            self.label(),
            output_path.display()
        );

        let mut cmd = Command::new(&self.binary);
        cmd.arg("save").arg(reference).arg("-o").arg(output_path);

        let output = cmd.output().await.map_err(|err| {
            PusherError::push_error(format!(
                "Failed to execute {} save command: {}",
                self.label(),
                err
            ))
        })?;

        if !output.status.success() {
            return Err(PusherError::push_error(format!(
                "{} save command failed for {}: {}",
                self.label(),
                reference,
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }
}

/// Represents a local image entry sourced from the runtime JSON output.
#[derive(Debug, Clone)]
struct LocalImage {
    reference: String,
    size: String,
    created: String,
}

impl LocalImage {
    fn from_value(value: Value) -> Option<Self> {
        let repo = value["Repository"].as_str().unwrap_or("").trim();
        let tag = value["Tag"].as_str().unwrap_or("").trim();
        if repo.is_empty() || tag.is_empty() || repo == "<none>" || tag == "<none>" {
            return None;
        }
        let reference = format!("{}:{}", repo, tag);
        Some(Self {
            reference,
            size: value["Size"].as_str().unwrap_or("").to_string(),
            created: value["CreatedSince"].as_str().unwrap_or("").to_string(),
        })
    }
}

/// Entry point for the `save` command that detects a runtime, gathers selections, and exports tars.
pub async fn run_save(args: SaveArgs) -> Result<(), PusherError> {
    let runtime = ContainerRuntime::detect(args.runtime)?;
    println!(
        "🐳 Using {} runtime located at {}",
        runtime.label(),
        runtime.binary.display()
    );

    let mut selected_images = args.images.clone();
    if selected_images.is_empty() {
        let available = runtime.list_images().await?;
        if available.is_empty() {
            return Err(PusherError::push_error(format!(
                "No local images detected via {}",
                runtime.label()
            )));
        }
        selected_images = prompt_for_images(&available).await?;
    }

    if selected_images.is_empty() {
        return Err(PusherError::push_error("No images selected for save"));
    }

    let output_dir = args
        .output_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    tokio::fs::create_dir_all(&output_dir).await?;

    let mut completed = 0usize;
    for reference in selected_images {
        let tar_path = build_tar_path(&output_dir, &reference);
        if tar_path.exists() && !args.force {
            if !confirm_overwrite(&tar_path).await? {
                println!(
                    "⚠️  Skipping {} because {} already exists",
                    reference,
                    tar_path.display()
                );
                continue;
            }
        }

        runtime.save_image(&reference, &tar_path).await?;
        println!("💾 Saved {} to {}", reference, tar_path.display());
        completed += 1;
    }

    if completed == 0 {
        println!("⚠️  Nothing was exported; all images skipped");
    } else {
        println!(
            "✅ Exported {} image(s) to {}",
            completed,
            output_dir.display()
        );
    }

    Ok(())
}

/// Normalizes `repo[:tag]` references into filesystem-safe tar filenames.
fn build_tar_path(dir: &Path, reference: &str) -> PathBuf {
    let sanitized = reference
        .replace('/', "_")
        .replace(':', "_")
        .replace('@', "_");
    dir.join(format!("{}.tar", sanitized))
}

/// Prompt helper that asks before overwriting an existing tar unless `--force` is set.
async fn confirm_overwrite(path: &Path) -> Result<bool, PusherError> {
    let prompt = format!("{} exists. Overwrite? [y/N]: ", path.display());
    prompt_yes_no(&prompt).await
}

/// Renders an interactive selector so the user can choose which images to export.
async fn prompt_for_images(candidates: &[LocalImage]) -> Result<Vec<String>, PusherError> {
    println!("🔍 Detected {} local images:", candidates.len());
    for (idx, image) in candidates.iter().enumerate() {
        println!(
            "  [{}] {:<40} {:<12} {:<12}",
            idx + 1,
            image.reference,
            image.size,
            image.created
        );
    }

    let input =
        prompt_text("Select images to save (numbers, comma separated, '*' for all): ").await?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if trimmed == "*" || trimmed.eq_ignore_ascii_case("all") {
        return Ok(candidates.iter().map(|c| c.reference.clone()).collect());
    }

    let mut selections = Vec::new();
    for token in trimmed
        .split(',')
        .map(|value| value.trim())
        .filter(|v| !v.is_empty())
    {
        if let Ok(index) = token.parse::<usize>() {
            if index == 0 || index > candidates.len() {
                return Err(PusherError::push_error(format!(
                    "Selection '{}' is out of range",
                    token
                )));
            }
            selections.push(candidates[index - 1].reference.clone());
            continue;
        }

        if let Some(image) = candidates.iter().find(|img| img.reference == token) {
            selections.push(image.reference.clone());
        } else {
            return Err(PusherError::push_error(format!(
                "Unable to match image '{}'",
                token
            )));
        }
    }

    Ok(selections)
}

/// Runs a blocking stdin prompt on a dedicated thread and returns the typed response.
async fn prompt_text(prompt: &str) -> Result<String, PusherError> {
    let prompt = prompt.to_string();
    task::spawn_blocking(move || {
        use std::io::{self, Write};
        print!("{}", prompt);
        io::stdout().flush().map_err(PusherError::IoError)?;
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(PusherError::IoError)?;
        Ok(input)
    })
    .await
    .map_err(|err| PusherError::push_error(format!("Prompt failed: {}", err)))?
}

/// Convenience yes/no wrapper built on top of `prompt_text`.
async fn prompt_yes_no(prompt: &str) -> Result<bool, PusherError> {
    let answer = prompt_text(prompt).await?;
    let normalized = answer.trim().to_lowercase();
    Ok(normalized == "y" || normalized == "yes")
}
