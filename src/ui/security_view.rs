use super::*;
use proxmark_studio::sealed;
impl Studio {
    pub(super) fn lock_library(&mut self) {
        if self.settings.sealed_library.is_some() && !self.library_passphrase.is_empty() {
            if let Err(e) = self.persist_ui() {
                self.security_message = format!("Library could not be saved before locking: {e}");
                return;
            }
            self.settings.cards.clear();
            self.settings.emulator_images.clear();
            self.library_passphrase.zeroize();
        }
    }
    pub(super) fn security_settings(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading("Private files");
        ui.label("Encrypt saved card and emulator libraries with a passphrase. Authentication keys remain in the OS key store. Connection profiles, credential profiles and operation history are separate from this library protection.");
        if self.settings.sealed_library.is_none() {
            ui.label("Library passphrase · minimum 12 characters");
            ui.add(egui::TextEdit::singleline(&mut *self.library_unlock_input).password(true));
            ui.label("Confirm passphrase");
            ui.add(egui::TextEdit::singleline(&mut *self.passphrase_confirmation).password(true));
            if ui.button("Encrypt saved libraries").clicked() {
                self.security_message=(||->Result<String,String>{if *self.library_unlock_input!=*self.passphrase_confirmation{return Err("Passphrases do not match".into());}let data=sealed::Library{cards:self.settings.cards.clone(),images:self.settings.emulator_images.clone()};let bytes=Zeroizing::new(serde_json::to_vec(&data).map_err(|e|e.to_string())?);let envelope=sealed::seal(&bytes,&self.library_unlock_input)?;self.settings.sealed_library=Some(envelope);self.library_passphrase=self.library_unlock_input.clone();if let Err(e)=self.persist_ui(){self.settings.sealed_library=None;self.library_passphrase.zeroize();return Err(e);}self.library_unlock_input.zeroize();self.passphrase_confirmation.zeroize();Ok("Saved libraries encrypted. Keep the passphrase: Studio cannot recover it. Existing exported files or backups are unchanged.".into())})().unwrap_or_else(|e|e);
            }
        } else if self.library_passphrase.is_empty() {
            ui.label("Libraries are locked");
            ui.add(
                egui::TextEdit::singleline(&mut *self.library_unlock_input)
                    .password(true)
                    .hint_text("Library passphrase"),
            );
            if ui.button("Unlock libraries").clicked() {
                self.security_message = (|| -> Result<String, String> {
                    let plain = sealed::open(
                        self.settings.sealed_library.as_ref().unwrap(),
                        &self.library_unlock_input,
                    )?;
                    let library: sealed::Library = serde_json::from_slice(&plain)
                        .map_err(|_| "Encrypted library has an invalid structure")?;
                    if library.images.len() > 16 {
                        return Err("Encrypted library exceeds the image limit".into());
                    }
                    for image in &library.images {
                        image.validate()?;
                    }
                    self.settings.cards = library.cards;
                    self.settings.emulator_images = library.images;
                    self.library_passphrase = self.library_unlock_input.clone();
                    self.library_unlock_input.zeroize();
                    Ok("Libraries unlocked for this session".into())
                })()
                .unwrap_or_else(|e| e);
            }
        } else {
            ui.label("Saved libraries are encrypted and unlocked for this session.");
            if ui.button("Lock libraries now").clicked() {
                self.lock_library();
            }
        }
        ui.separator();
        ui.label("Encrypted image import/export passphrase");
        ui.add(egui::TextEdit::singleline(&mut *self.file_passphrase).password(true));
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.bench.dump.is_some(),
                    egui::Button::new("Export encrypted image…"),
                )
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("card-image.pm3enc")
                    .save_file()
                {
                    let image = proxmark_studio::magic::image(
                        self.bench.dump.clone().unwrap(),
                        self.bench
                            .origin_simulated
                            .unwrap_or(self.simulation.is_some()),
                    );
                    self.security_message = serde_json::to_vec(&image)
                        .map_err(|e| e.to_string())
                        .and_then(|v| {
                            sealed::save(&path, &Zeroizing::new(v), &self.file_passphrase)
                        })
                        .map(|()| "Encrypted image exported".into())
                        .unwrap_or_else(|e| e);
                }
            }
            if ui.button("Import encrypted image…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Encrypted Studio image", &["pm3enc"])
                    .pick_file()
                {
                    self.security_message = sealed::load(&path, &self.file_passphrase)
                        .and_then(|v| {
                            let image: proxmark_studio::emulator::Image =
                                serde_json::from_slice(&v).map_err(|_| {
                                    "Encrypted file does not contain a Studio image"
                                })?;
                            image.dump.validate()?;
                            self.bench.geometry = image.dump.geometry;
                            self.bench.uid = image.dump.uid.clone();
                            self.bench.editor = command::encode(&image.dump.bytes);
                            self.bench.origin_simulated = Some(image.simulated);
                            self.bench.dump = Some(image.dump);
                            self.bench.draft_history.reset();
                            Ok("Image decrypted into Card memory; provenance retained".into())
                        })
                        .unwrap_or_else(|e| e);
                }
            }
        });
        ui.label("AES-256-GCM authenticates the contents; Argon2id derives the encryption key. No passphrase is saved. This does not erase earlier plaintext copies or provide secure disk deletion.");
        if !self.security_message.is_empty() {
            ui.label(&self.security_message);
        }
    }
}
