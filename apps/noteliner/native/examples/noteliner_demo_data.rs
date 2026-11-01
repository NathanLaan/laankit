//! Builds a throwaway NoteLiner project for screenshots and manual testing:
//! a git repository with nested notes, tags, wikilinks, a template and an
//! image attachment, plus `recent-projects.json` under
//! `$XDG_CONFIG_HOME/NoteLiner` so it shows on the open screen. Never point
//! it at your real config.
//!
//! ```bash
//! export XDG_CONFIG_HOME=$(mktemp -d)
//! cargo run -p noteliner-native --example noteliner_demo_data -- /tmp/nl-demo
//! cargo run -p noteliner-native            # or LAANKIT_PROJECT=/tmp/nl-demo
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use noteliner_core::git::NoteGit;
use noteliner_core::project::Project;
use serde_json::json;

/// A small gradient PNG, for the attachment.
fn png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(96, 48, |x, y| {
        image::Rgba([(x * 2) as u8, 120, (y * 4) as u8, 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .expect("encode png");
    out.into_inner()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: noteliner_demo_data <project dir>"),
    );
    let Some(config) = std::env::var_os("XDG_CONFIG_HOME") else {
        anyhow::bail!("set XDG_CONFIG_HOME to a temp folder first");
    };
    std::fs::create_dir_all(&dir)?;
    let mut project = Project::new(NoteGit::new(Arc::new(|msg| println!("{msg}"))));
    project.init(&dir, None).await?;
    project
        .set_git_config("Demo User", "demo@localhost")
        .await?;

    let welcome = project
        .create_file(
            "Welcome",
            &json!(["guide"]),
            Some(
                "# Welcome\n\nThis is a **demo** project for the native NoteLiner.\n\n## Links\n\n- See [[Meeting Notes]] and [[Reading List|the reading list]].\n- A missing note: [[Someday]].\n\n## Code\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n\n| Feature | Native |\n|---|---|\n| Tree | Done |\n| Preview | Done |\n"
                    .into(),
            ),
            None,
        )
        .await?;
    let welcome_id = welcome["id"].as_str().unwrap_or_default().to_string();
    project
        .create_file(
            "Meeting Notes",
            &json!(["work"]),
            Some("# Meeting Notes\n\n## Agenda\n\n1. Native port status\n2. Next wave\n\n### Actions\n\n- [x] Ship ThreadLiner\n- [ ] Ship NoteLiner\n".into()),
            Some(&welcome_id),
        )
        .await?;
    project
        .create_file(
            "Reading List",
            &json!(["reading", "work"]),
            Some(
                "# Reading List\n\n> Read more.\n\n- *Chokepoint Capitalism*\n- *Red Team Blues*\n"
                    .into(),
            ),
            Some(&welcome_id),
        )
        .await?;
    let ideas = project
        .create_file(
            "Ideas",
            &json!([]),
            Some("# Ideas\n\nNothing tagged here.\n".into()),
            None,
        )
        .await?;
    let ideas_id = ideas["id"].as_str().unwrap_or_default().to_string();
    let att = project
        .add_attachment(&ideas_id, &png(), "gradient.png")
        .await?;
    let img = att["filename"].as_str().unwrap_or_default();
    let filename = ideas["filename"].as_str().unwrap_or_default().to_string();
    project
        .write_file(
            &filename,
            &format!("# Ideas\n\nNothing tagged here, but an image:\n\n![gradient](./_attachments/{img})\n"),
        )
        .await?;
    let talk = project
        .create_file(
            "Product Talk",
            &json!(["work"]),
            Some(
                "# Native NoteLiner\n\nA demo deck\n\n## Why native\n\n- One Rust binary\n- No webview\n- Six themes\n\n<!-- notes\nMention the frame times.\n-->\n\n## The numbers\n\n| Note size | Typing (p95) |\n|---|---|\n| 5,000 lines | 2–3 ms |\n| 20,000 lines | 15 ms |\n\n## Slides, too\n\n![gradient](./_attachments/IMG)\n"
                    .replace("IMG", img),
            ),
            None,
        )
        .await?;
    project
        .set_presentation(
            talk["filename"].as_str().unwrap_or_default(),
            &json!({
                "theme": "dark",
                "aspect": "16:9",
                "slideLevel": 2,
                "header": "",
                "footer": "{{title}} · {{date}}",
                "slideNumbers": true,
                "firstSlideTitle": true,
            }),
        )
        .await?;
    std::fs::create_dir_all(dir.join("_templates"))?;
    std::fs::write(
        dir.join("_templates/daily-log.md"),
        "# {{title}}\n\n{{date}}\n\n- \n",
    )?;

    let user_data = PathBuf::from(config).join("NoteLiner");
    std::fs::create_dir_all(&user_data)?;
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    laankit_core::json::write(
        &user_data.join("recent-projects.json"),
        &json!([{ "path": dir.display().to_string(), "name": name, "openedAt": noteliner_core::project::now_iso() }]),
    )?;
    println!("project: {}", dir.display());
    Ok(())
}
