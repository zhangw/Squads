use crate::Message;
use crate::widgets::{gif::Gif, viewport::ViewportHandler};
use bytes::Bytes;
use directories::ProjectDirs;
use iced::widget::{container, image, space};
use iced::{Color, ContentFit, Element};
use std::str::FromStr;
use std::{
    fs::{File, create_dir_all},
    io::Write,
    path::{Path, PathBuf},
};
pub fn save_cached_image(identifier: String, extension: &str, bytes: Bytes) {
    // Security: only write validated media within the size budget.
    if bytes.len() > crate::security::MAX_MEDIA_BYTES
        || !crate::security::validate_media_bytes(extension, &bytes)
    {
        eprintln!("Security: rejecting invalid media for {}", identifier);
        return;
    }
    let project_dirs = ProjectDirs::from("", "ianterzo", "squads");

    let mut cache_dir = project_dirs.unwrap().cache_dir().to_path_buf();
    cache_dir.push("image-cache");

    if !cache_dir.exists() {
        create_dir_all(&cache_dir).expect("Failed to create image-cache directory");
    }

    enforce_cache_budget(&cache_dir);

    let target = cache_dir.join(format!("{}.{}", identifier, extension));
    if target.exists() {
        return;
    }
    // Atomic write: temp file in the same directory, then rename.
    let tmp = cache_dir.join(format!(".{}.{}.tmp", identifier, std::process::id()));
    let mut ok = false;
    if let Ok(mut file) = File::create(&tmp) {
        ok = file.write_all(&bytes).is_ok() && file.sync_all().is_ok();
    }
    if ok {
        let _ = std::fs::rename(&tmp, &target);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Keep the media cache within a total budget; evict oldest files first.
fn enforce_cache_budget(cache_dir: &Path) {
    const MAX_TOTAL: u64 = 256 * 1024 * 1024; // 256 MiB
    let mut entries: Vec<(std::time::SystemTime, PathBuf, u64)> = Vec::new();
    let mut total: u64 = 0;
    if let Ok(rd) = std::fs::read_dir(cache_dir) {
        for e in rd.flatten() {
            if let Ok(md) = e.metadata() {
                if md.is_file() {
                    total += md.len();
                    let t = md.modified().unwrap_or(std::time::UNIX_EPOCH);
                    entries.push((t, e.path(), md.len()));
                }
            }
        }
    }
    if total <= MAX_TOTAL {
        return;
    }
    entries.sort_by_key(|(t, _, _)| *t);
    for (_, p, len) in entries {
        if total <= MAX_TOTAL {
            break;
        }
        if std::fs::remove_file(&p).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

pub fn c_cached_image<'a>(
    identifier: String,
    on_enter_unique: Message,
    image_width: f32,
    image_height: f32,
    border_radius: f32,
) -> Element<'a, Message> {
    let mut team_picture = container(
        ViewportHandler::new(space()).on_enter_unique(identifier.clone(), on_enter_unique.clone()),
    )
    .style(|_| container::Style {
        background: Some(
            Color::from_str("#b8b4b4")
                .expect("Background color is invalid.")
                .into(),
        ),

        ..Default::default()
    })
    .width(image_width)
    .height(image_height);

    let project_dirs = ProjectDirs::from("", "ianterzo", "squads");
    let mut image_path = project_dirs.unwrap().cache_dir().to_path_buf();
    image_path.push("image-cache");
    image_path.push(format!("{}.jpeg", identifier));

    if Path::new(&image_path).exists() {
        team_picture = container(
            ViewportHandler::new(
                image(image_path)
                    .content_fit(ContentFit::Fill)
                    .width(image_width)
                    .height(image_height)
                    .border_radius(border_radius),
            )
            .on_enter_unique(identifier, on_enter_unique),
        )
    }

    team_picture.into()
}

pub fn c_cached_gif<'a>(
    identifier: String,
    on_enter_unique: Message,
    image_width: f32,
    image_height: f32,
) -> Element<'a, Message> {
    let mut team_picture = container(
        ViewportHandler::new(space()).on_enter_unique(identifier.clone(), on_enter_unique.clone()),
    )
    .style(|_| container::Style {
        background: Some(
            Color::from_str("#b8b4b4")
                .expect("Background color is invalid.")
                .into(),
        ),

        ..Default::default()
    })
    .width(image_width)
    .height(image_height);
    let project_dirs = ProjectDirs::from("", "ianterzo", "squads");

    let mut image_path = project_dirs.unwrap().cache_dir().to_path_buf();
    image_path.push("image-cache");
    image_path.push(format!("{}.gif", identifier));
    if Path::new(&image_path).exists() {
        team_picture = container(
            ViewportHandler::new(
                Gif::new(image_path)
                    .content_fit(ContentFit::Fill)
                    .width(image_width.into())
                    .height(image_height.into()),
            )
            .on_enter_unique(identifier, on_enter_unique),
        )
    }

    team_picture.into()
}
