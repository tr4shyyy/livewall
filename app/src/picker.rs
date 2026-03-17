use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

pub fn choose_video_file(initial_dir: &Path) -> Result<Option<PathBuf>> {
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; \
        $dlg = New-Object System.Windows.Forms.OpenFileDialog; \
        $dlg.Filter = 'Video files (*.mp4;*.webm;*.mkv;*.mov)|*.mp4;*.webm;*.mkv;*.mov|All files (*.*)|*.*'; \
        $dlg.InitialDirectory = '{initial_dir}'; \
        $dlg.Multiselect = $false; \
        if ($dlg.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ [Console]::Out.Write($dlg.FileName) }}",
        initial_dir = ps_single_quote_escape(&initial_dir.to_string_lossy())
    );

    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-STA",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .output()
        .context("failed to launch file picker")?;

    if !output.status.success() {
        return Ok(None);
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Ok(None);
    }

    Ok(Some(PathBuf::from(path)))
}

fn ps_single_quote_escape(value: &str) -> String {
    value.replace('\'', "''")
}
