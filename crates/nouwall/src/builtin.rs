//! Built-in sample wallpapers, installed once into the library. Samples the
//! user deletes stay deleted (tracked in `.builtin`). Release zips may also
//! ship a `samples\` folder next to the executable (e.g. a demo video).

use std::collections::BTreeSet;
use std::fs;

use nouwall_core::{Config, Kind, Library, Manifest, MonitorTarget};

use crate::{commands, paths};

struct Sample {
    id: &'static str,
    title: &'static str,
    kind: Kind,
    file: &'static str,
    bytes: &'static [u8],
    description: &'static str,
    cost: Option<u8>,
}

const SAMPLES: &[Sample] = &[
    Sample {
        id: "aurora",
        title: "Aurora",
        kind: Kind::Shader,
        file: "aurora.glsl",
        bytes: include_bytes!("../assets/shaders/aurora.glsl"),
        description: "Aurora ribbons drifting over a starry sky (GLSL).",
        cost: Some(2),
    },
    Sample {
        id: "glass-blobs",
        title: "Glass Blobs",
        kind: Kind::Shader,
        file: "glass-blobs.glsl",
        bytes: include_bytes!("../assets/shaders/glass-blobs.glsl"),
        description: "Soft glass gradients that follow the cursor (GLSL).",
        cost: Some(1),
    },
    Sample {
        id: "starfield",
        title: "Starfield",
        kind: Kind::Shader,
        file: "starfield.wgsl",
        bytes: include_bytes!("../assets/shaders/starfield.wgsl"),
        description: "Layers of stars rushing towards the screen (WGSL).",
        cost: Some(1),
    },
    Sample {
        id: "jam-kaca",
        title: "Jam Kaca",
        kind: Kind::Web,
        file: "index.html",
        bytes: include_bytes!("../assets/web/clock.html"),
        description: "Clock and date on a glass card over an animated background (HTML).",
        cost: None,
    },
];

const MARKER: &str = ".builtin";

fn read_marker(lib: &Library) -> BTreeSet<String> {
    fs::read_to_string(lib.root().join(MARKER))
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Install missing samples; on a fresh install also assign the first one.
pub fn install(lib: &Library, config: &mut Config) -> bool {
    let _ = fs::create_dir_all(lib.root());
    let mut seen = read_marker(lib);
    let fresh = seen.is_empty();
    let mut changed = false;

    for s in SAMPLES {
        if seen.contains(s.id) {
            continue;
        }
        let manifest = Manifest {
            title: s.title.into(),
            kind: s.kind,
            entry: s.file.into(),
            author: "nouwall".into(),
            description: s.description.into(),
            video: None,
            shader_cost: s.cost,
            display: Default::default(),
        };
        match lib.create(s.id, &manifest, &[(s.file, s.bytes)]) {
            Ok(_) => log::info!("sample {} installed", s.id),
            Err(e) => log::warn!("sample {}: {e}", s.id),
        }
        seen.insert(s.id.into());
        changed = true;
    }

    // Extra samples shipped next to the executable.
    if let Some(dir) = paths::exe_dir().map(|d| d.join("samples")) {
        if let Ok(files) = fs::read_dir(&dir) {
            for f in files.flatten() {
                let key = format!("file:{}", f.file_name().to_string_lossy());
                if seen.contains(&key) {
                    continue;
                }
                match commands::import_path(lib, &f.path()) {
                    Ok(e) => log::info!("sample {} imported from {}", e.id, dir.display()),
                    Err(e) => log::warn!("sample {}: {e}", f.path().display()),
                }
                seen.insert(key);
                changed = true;
            }
        }
    }

    if changed {
        let text: Vec<_> = seen.into_iter().collect();
        let _ = fs::write(lib.root().join(MARKER), text.join("\n"));
    }
    if fresh && config.assignments.is_empty() {
        config.assign(MonitorTarget::ALL, SAMPLES[0].id);
        return true;
    }
    false
}
