//! A suggested icon for a project folder, from the names of the files in its
//! top level (never their contents). The user's own choice (kept by the UI)
//! always wins over this.

use std::path::Path;

/// At most this many entries are looked at (big folders like ~/Downloads).
const MAX_ENTRIES: usize = 600;

/// Icon id for the folder (matching `src/lib/folderIcons.ts`), or None for the plain folder.
pub fn detect(dir: &Path) -> Option<&'static str> {
    let names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .take(MAX_ENTRIES)
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect();
    kind(&names)
}

fn kind(names: &[String]) -> Option<&'static str> {
    let has = |n: &str| names.iter().any(|x| x == n);
    let ext = |e: &str| names.iter().any(|x| x.rsplit_once('.').is_some_and(|(stem, x)| !stem.is_empty() && x == e));
    let any = |list: &[&str]| list.iter().any(|n| has(n));
    // Most specific first: a phone app or a notebook project usually has a package.json or requirements.txt too.
    if has("pubspec.yaml") || ext("xcodeproj") || ext("xcworkspace") || has("androidmanifest.xml") || (has("android") && has("ios")) {
        return Some("mobile");
    }
    if has("project.godot") || ext("uproject") || (has("assets") && has("projectsettings")) {
        return Some("game");
    }
    if any(&["dvc.yaml", "mlproject", "modelfile"]) || ["safetensors", "gguf", "onnx", "pt", "ckpt"].iter().any(|e| ext(e)) {
        return Some("ai");
    }
    if ext("ipynb") || ext("rproj") || ext("mlx") || any(&["environment.yml", "environment.yaml", "project.toml"]) {
        return Some("science");
    }
    if ext("tex") || ext("bib") {
        return Some("doc");
    }
    if any(&["package.json", "index.html", "deno.json", "deno.jsonc", "composer.json"]) {
        return Some("web");
    }
    if ext("tf") || any(&["serverless.yml", "wrangler.toml", "cdk.json", "pulumi.yaml", "fly.toml"]) {
        return Some("cloud");
    }
    if any(&["dockerfile", "docker-compose.yml", "docker-compose.yaml", "compose.yml", "compose.yaml"]) {
        return Some("server");
    }
    if any(&["prisma", "schema.sql", "migrations"]) || ext("sql") {
        return Some("database");
    }
    if any(&[
        "cargo.toml", "go.mod", "pom.xml", "build.gradle", "build.gradle.kts", "cmakelists.txt", "makefile", "meson.build",
        "pyproject.toml", "setup.py", "requirements.txt", "gemfile", "mix.exs", "package.swift", "build.zig", "stack.yaml", "dune-project",
    ]) || ext("csproj") || ext("sln") || ext("cabal")
    {
        return Some("code");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(names: &[&str]) -> Option<&'static str> {
        kind(&names.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>())
    }

    #[test]
    fn picks_the_most_specific_kind() {
        assert_eq!(k(&["package.json", "src-tauri", "README.md"]), Some("web"));
        assert_eq!(k(&["package.json", "app.json", "android", "ios"]), Some("mobile"));
        assert_eq!(k(&["Runner.xcodeproj", "Podfile"]), Some("mobile"));
        assert_eq!(k(&["requirements.txt", "analysis.ipynb"]), Some("science"));
        assert_eq!(k(&["Cargo.toml", "src"]), Some("code"));
        assert_eq!(k(&["main.tex", "refs.bib", "Makefile"]), Some("doc"));
        assert_eq!(k(&["Dockerfile", "go.mod"]), Some("server"));
        assert_eq!(k(&["main.tf"]), Some("cloud"));
        assert_eq!(k(&["model.safetensors", "pyproject.toml"]), Some("ai"));
        assert_eq!(k(&["project.godot"]), Some("game"));
        // ".sql" alone is a hidden file name, not an extension.
        assert_eq!(k(&[".sql", "notes.txt", "photo.jpg"]), None);
        assert_eq!(k(&[]), None);
    }

    #[test]
    fn reads_a_real_folder() {
        let dir = std::env::temp_dir().join(format!("wren-folder-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(detect(&dir), None);
        std::fs::write(dir.join("Cargo.toml"), "").unwrap();
        assert_eq!(detect(&dir), Some("code"));
        assert_eq!(detect(&dir.join("missing")), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
