use gdrive::list_my_files;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Ok(list_my_files().await)
}
