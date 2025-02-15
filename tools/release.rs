use std::fs;
use std::process::Command;
use toml_edit::DocumentMut;
use toml_edit::Item;

fn get_commit_history(previous_tag: &str) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(&[
            "log",
            "--pretty=format:- %s",
            &format!("{}..HEAD", previous_tag),
        ])
        .output()?;

    Ok(String::from_utf8(output.stdout)?)
}

fn get_latest_tag() -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(&["describe", "--tags", "--abbrev=0"])
        .output()?;

    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read current Cargo.toml
    let cargo_content = fs::read_to_string("Cargo.toml")?;
    let mut doc = cargo_content.parse::<DocumentMut>()?;

    // Get current version
    let current_version = doc["package"]["version"]
        .as_str()
        .expect("Could not find version in Cargo.toml");

    // Rest of the code remains the same...
    // Ask for new version
    println!("Current version is: {}", current_version);
    println!("Enter new version:");
    let mut new_version = String::new();
    std::io::stdin().read_line(&mut new_version)?;
    let new_version = new_version.trim();

    // Update Cargo.toml
    doc["package"]["version"] = Item::from(new_version);
    fs::write("Cargo.toml", doc.to_string())?;

    // Get the latest tag for commit history
    let previous_tag = get_latest_tag()?;
    let commit_history = get_commit_history(&previous_tag)?;

    // Git commands
    let commands = [
        ("git add Cargo.toml", "Failed to stage Cargo.toml"),
        (
            &format!("git commit -m \"Bump version to {}\"", new_version),
            "Failed to commit version bump",
        ),
        (
            &format!("git tag -a v{} -m \"Version {}\"", new_version, new_version),
            "Failed to create tag",
        ),
        ("git push", "Failed to push commits"),
        ("git push --tags", "Failed to push tags"),
        ("cargo publish", "Failed to publish to crates.io"),
    ];

    for (cmd, error_msg) in commands.iter() {
        let status = Command::new("sh").arg("-c").arg(cmd).status()?;

        if !status.success() {
            return Err(error_msg.to_string().into());
        }
    }

    // Create GitHub release
    let create_release = Command::new("gh")
        .args(&[
            "release",
            "create",
            &format!("v{}", new_version),
            "--title",
            &format!("v{}", new_version),
            "--notes",
            &commit_history,
        ])
        .status()?;

    if !create_release.success() {
        return Err("Failed to create GitHub release".into());
    }

    println!("Successfully released version {}", new_version);
    Ok(())
}
