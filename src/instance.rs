use crate::install_frame::InstanceSource;
use crate::{install, style, update, Message};
use anyhow::Result;
use chrono::{DateTime, Local};
use iced::{button, Align, Button, Column, Element, Row, Text};
use platform_dirs::{AppDirs, AppUI};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;
use std::{fs, thread};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum InstanceType {
    MacOS,
    Windows,
    Linux,
    AppImage,
    Unknown,
}

impl InstanceType {
    pub fn archive(self) -> Option<&'static str> {
        match self {
            InstanceType::MacOS => Some("EndlessSky-macOS-continuous.zip"),
            InstanceType::Windows => Some("EndlessSky-win64-continuous.zip"),
            InstanceType::Linux => Some("endless-sky-x86_64-continuous.tar.gz"),
            InstanceType::AppImage => Some("endless-sky-x86_64-continuous.AppImage"),
            InstanceType::Unknown => None,
        }
    }

    pub fn executable(self) -> Option<&'static str> {
        match self {
            InstanceType::MacOS => Some("Endless Sky.app/Content/MacOS/Endless Sky"),
            InstanceType::Windows => Some("EndlessSky.exe"),
            InstanceType::Linux => Some("endless-sky"),
            InstanceType::AppImage => Some("endless-sky-x86_64-continuous.AppImage"),
            InstanceType::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    #[serde(skip)]
    play_button: button::State,
    #[serde(skip)]
    update_button: button::State,
    #[serde(skip)]
    delete_button: button::State,
    pub path: PathBuf,
    pub executable: PathBuf,
    pub name: String,
    pub instance_type: InstanceType,
    pub source: InstanceSource,
}

impl Default for Instance {
    fn default() -> Self {
        Instance {
            play_button: button::State::default(),
            update_button: button::State::default(),
            delete_button: button::State::default(),
            path: Default::default(),
            executable: Default::default(),
            name: Default::default(),
            instance_type: InstanceType::Unknown,
            source: InstanceSource::default(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum InstanceMessage {
    Play,
    Update,
    Delete,
}

impl Instance {
    pub fn new(
        path: PathBuf,
        executable: PathBuf,
        name: String,
        instance_type: InstanceType,
        source: InstanceSource,
    ) -> Self {
        Instance {
            play_button: button::State::default(),
            update_button: button::State::default(),
            delete_button: button::State::default(),
            path,
            executable,
            name,
            instance_type,
            source,
        }
    }

    pub fn update(&mut self, message: InstanceMessage) -> iced::Command<Message> {
        match message {
            InstanceMessage::Play => iced::Command::perform(
                perform_play(
                    self.path.clone(),
                    self.executable.clone(),
                    self.name.clone(),
                ),
                Message::Dummy,
            ),
            InstanceMessage::Update => iced::Command::perform(
                perform_update(self.path.clone(), self.instance_type, self.source.clone()),
                Message::Dummy,
            ),
            InstanceMessage::Delete => {
                iced::Command::perform(delete(self.path.clone()), Message::Deleted)
            }
        }
    }

    pub fn view(&mut self) -> Element<InstanceMessage> {
        Row::new()
            .spacing(10)
            .padding(10)
            .align_items(Align::Start)
            .push(
                Column::new().push(Text::new(&self.name).size(24)).push(
                    Text::new(format!(
                        "Source: {} - {}",
                        self.source.r#type, self.source.identifier
                    ))
                    .size(10),
                ),
            )
            .push(
                Row::new()
                    .spacing(10)
                    .push(
                        Button::new(&mut self.play_button, style::play_icon())
                            .style(style::Button::Icon)
                            .on_press(InstanceMessage::Play),
                    )
                    .push(
                        Button::new(&mut self.update_button, style::update_icon())
                            .style(style::Button::Icon)
                            .on_press(InstanceMessage::Update),
                    )
                    .push(
                        Button::new(&mut self.delete_button, style::delete_icon())
                            .style(style::Button::Destructive)
                            .on_press(InstanceMessage::Delete),
                    ),
            )
            .into()
    }
}

pub async fn perform_install(
    path: PathBuf,
    name: String,
    instance_type: InstanceType,
    instance_source: InstanceSource,
) -> Option<Instance> {
    match install::install(path, name, instance_type, instance_source) {
        Ok(instance) => Some(instance),
        Err(e) => {
            error!("Install failed: {:#}", e);
            None
        }
    }
}

pub async fn delete(path: PathBuf) -> Option<PathBuf> {
    match std::fs::remove_dir_all(&path) {
        Ok(_) => {
            info!("Removed {}", path.to_string_lossy());
            Some(path)
        }
        Err(_) => {
            error!("Failed to remove {:#}", path.to_string_lossy());
            None
        }
    }
}

pub async fn perform_update(path: PathBuf, instance_type: InstanceType, source: InstanceSource) {
    // Yes, this is terrible. Sue me. Bitar's objects don't implement Send, and i cannot figure out
    // how to use them in the default executor (which is multithreaded, presumably). Since we don't
    // need any sort of feedback other than logs, we can just update in new, single-threaded runtime.
    thread::spawn(move || {
        match tokio::runtime::Runtime::new() {
            Ok(mut runtime) => {
                if let Err(e) =
                    runtime.block_on(update::update_instance(path, instance_type, source))
                {
                    error!("Failed to update instance: {:#}", e)
                }
            }
            Err(e) => error!("Failed to spawn tokio runtime: {}", e),
        };
    });
}

pub async fn perform_play(path: PathBuf, executable: PathBuf, name: String) {
    if let Err(e) = play(path, executable, name).await {
        error!("Failed to run game: {}", e);
    }
}

pub async fn play(path: PathBuf, executable: PathBuf, name: String) -> Result<()> {
    let mut log_path = path;
    log_path.push("logs");
    fs::create_dir_all(&log_path)?;

    let time = DateTime::<Local>::from(SystemTime::now())
        .format("%F %H-%M-%S")
        .to_string();
    let mut out_path = log_path.clone();
    out_path.push(format!("{}.out", time));
    let mut out = File::create(out_path)?;

    let mut err_path = log_path.clone();
    err_path.push(format!("{}.err", time));
    let mut err = File::create(err_path)?;

    info!("Launching {}", name);
    match Command::new(&executable).output() {
        Ok(output) => {
            info!("{} exited with {}", name, output.status);
            out.write_all(&output.stdout)?;
            err.write_all(&output.stderr)?;
            info!(
                "Logfiles have been written to {}",
                log_path.to_string_lossy()
            );
            if !output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                error!("Stdout was: {}", stdout);
                error!("Stderr was: {}", stderr);
            }
        }
        Err(e) => error!("Error starting process: {}", e),
    };
    Ok(())
}

pub fn get_instances_dir() -> Option<PathBuf> {
    let mut dir = AppDirs::new(Some("ESLauncher2"), AppUI::Graphical)?.data_dir;
    dir.push("instances");
    Some(dir)
}

#[derive(Serialize, Deserialize)]
struct InstancesContainer(Vec<Instance>);

pub fn perform_save_instances(instances: Vec<Instance>) {
    if let Err(e) = save_instances(instances) {
        error!("Failed to save instances: {}", e);
    };
}

fn save_instances(instances: Vec<Instance>) -> Result<()> {
    let mut instances_file =
        get_instances_dir().ok_or_else(|| anyhow!("Failed to get Instances dir"))?;
    instances_file.push("instances.json");

    let file = fs::File::create(instances_file)?;

    serde_json::to_writer_pretty(file, &InstancesContainer(instances))?;
    Ok(())
}

pub fn load_instances() -> Result<Vec<Instance>> {
    let mut instances_file =
        get_instances_dir().ok_or_else(|| anyhow!("Failed to get Instances dir"))?;
    instances_file.push("instances.json");

    let file = fs::File::open(instances_file)?;

    let container: InstancesContainer = serde_json::from_reader(file)?;
    Ok(container.0)
}
