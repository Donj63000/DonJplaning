fn main() {
    if let Err(error) = don_jplaning::ui::run() {
        eprintln!("Erreur DonJplaning: {error}");
        std::process::exit(1);
    }
}
