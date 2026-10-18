//! The player's EQ: live bands and preamp, the Suggested EQ at the chosen strength, the optional Audio
//! analysis switch, and the saved headphone corrections.

use super::*;

impl Player {
    // ---- EQ ----

    pub fn toggle_eq(&self) {
        let mut this = *self;
        let e = *this.eq_enabled.peek();
        this.eq_enabled.set(!e);
        self.persist_eq_enabled();
    }

    pub(super) fn touch_eq(&self) {
        let mut t = self.user_touched_eq;
        t.set(true);
    }

    pub fn set_band_gain(&self, index: usize, db: f32) {
        self.touch_eq();
        let mut bands = self.eq_bands;
        {
            let mut w = bands.write();
            if let Some(b) = w.get_mut(index) {
                b.gain = db;
            }
        }
    }

    pub fn set_band_freq(&self, index: usize, hz: f32) {
        self.touch_eq();
        let mut bands = self.eq_bands;
        {
            let mut w = bands.write();
            if let Some(b) = w.get_mut(index) {
                b.freq = hz;
            }
        }
    }

    pub fn add_band(&self) {
        self.touch_eq();
        let mut bands = self.eq_bands;
        bands.write().push(LiveEqBand { freq: 1000.0, gain: 0.0, q: DEFAULT_EQ_Q });
    }

    pub fn remove_band(&self, index: usize) {
        self.touch_eq();
        let mut bands = self.eq_bands;
        if index < bands.peek().len() {
            bands.write().remove(index);
        }
    }

    pub fn set_preamp_gain(&self, db: f32) {
        self.touch_eq();
        let mut p = self.eq_preamp;
        p.set(db);
    }

    /// Resets the live EQ to the current track's AI-suggested bands (or the
    /// default flat layout) and resumes auto-seeding from future tracks.
    pub fn reset_eq_to_suggested(&self) {
        let mut this = *self;
        this.user_touched_eq.set(false);
        let analysis = this.analysis.peek().clone();
        let (bands, preamp) = this.suggested_for(&analysis);
        this.eq_bands.set(bands);
        this.eq_preamp.set(preamp);
    }

    /// The live bands and preamp for a track's analysis at the current strength (flat when there is none).
    pub(super) fn suggested_for(&self, analysis: &Option<AudioAnalysis>) -> (Vec<LiveEqBand>, f32) {
        match analysis {
            Some(a) => suggested_from(a, *self.eq_strength.peek()),
            None => (default_eq_bands(), 0.0),
        }
    }

    /// Changes the Suggested EQ strength (0-100) and re-seeds the sliders from the track's suggestion. With
    /// `persist` it is also saved to the settings (do that once, when a drag ends).
    pub fn set_eq_strength(&self, percent: u8, persist: bool) {
        let percent = percent.min(100);
        let mut this = *self;
        this.eq_strength.set(percent as f32 / 100.0);
        self.reset_eq_to_suggested();
        if persist {
            let mut shared = self.settings;
            let mut next = shared.peek().clone();
            next.eq_strength_percent = percent;
            shared.set(next.clone());
            dioxus::core::spawn_forever(async move {
                let _ = api::set_settings(next).await;
            });
        }
    }

    /// Makes `id` the headphone correction in use (None: no correction) without remembering it.
    pub(super) fn apply_headphone(&self, id: Option<i64>) {
        let mut this = *self;
        let profile = id
            .and_then(|id| this.headphones.peek().iter().find(|h| h.id == id).map(|h| parse_parametric_eq(&h.preset)))
            .filter(|p| !p.bands.is_empty());
        this.active_headphone.set(if profile.is_some() { id } else { None });
        this.correction_structure.set(true);
        if profile.is_some() {
            this.correction_enabled.set(true);
        }
        this.correction_profile.set(profile);
    }

    /// Whether the optional Audio analysis feature is on (the shared settings, which start with it on).
    pub fn audio_analysis_on(&self) -> bool {
        self.settings.peek().audio_analysis_enabled
    }

    /// Called when Audio analysis is switched off in Settings: the playing track drops its suggestion and
    /// levelling at once and plays as it is. (Switching it on takes effect from the next track.)
    pub fn audio_analysis_switched_off(&self) {
        let mut this = *self;
        this.analysis.set(None);
        this.next_info.set(None);
        self.reset_eq_to_suggested();
        cmd("setNorm", json!({"db": null}));
    }

    /// Forgets the saved Headphone corrections on screen (after the stored ones were deleted).
    pub fn forget_headphones(&self) {
        let mut this = *self;
        this.headphones.set(vec![]);
        this.apply_headphone(None);
    }

    /// Picks the headphone correction to use (None: none) and remembers the choice.
    pub fn select_headphone(&self, id: Option<i64>) {
        self.apply_headphone(id);
        let active = *self.active_headphone.peek();
        dioxus::core::spawn_forever(async move {
            let _ = api::set_active_headphone(active).await;
        });
    }

    pub(super) fn check_headphone(name: &str, text: &str) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("Give it a name, such as the earphone or headphone it is for.".into());
        }
        if parse_parametric_eq(text).bands.is_empty() {
            return Err("No filter lines found - paste an Equalizer APO/PEACE parametric EQ export (Preamp + Filter N lines).".into());
        }
        Ok(())
    }

    /// Saves a pasted parametric EQ export under `name` and starts using it. Err says what's wrong.
    pub async fn save_headphone(&self, name: String, text: String) -> Result<(), String> {
        Self::check_headphone(&name, &text)?;
        let id = api::save_headphone_profile(name.trim().to_string(), text).await?;
        let mut this = *self;
        if let Ok(list) = api::list_headphone_profiles().await {
            this.headphones.set(list);
        }
        self.select_headphone(Some(id));
        Ok(())
    }

    /// Changes a saved correction's name and/or filters; if it is the one in use, the new filters take effect now.
    pub async fn update_headphone(&self, id: i64, name: String, text: String) -> Result<(), String> {
        Self::check_headphone(&name, &text)?;
        api::update_headphone_profile(id, name.trim().to_string(), text).await?;
        let mut this = *self;
        if let Ok(list) = api::list_headphone_profiles().await {
            this.headphones.set(list);
        }
        if *this.active_headphone.peek() == Some(id) {
            this.apply_headphone(Some(id));
        }
        Ok(())
    }

    pub fn delete_headphone(&self, id: i64) {
        let mut this = *self;
        this.headphones.write().retain(|h| h.id != id);
        if *this.active_headphone.peek() == Some(id) {
            self.select_headphone(None);
        }
        dioxus::core::spawn_forever(async move {
            let _ = api::delete_headphone_profile(id).await;
        });
    }

    pub fn set_correction_enabled(&self, enabled: bool) {
        let mut this = *self;
        this.correction_enabled.set(enabled);
    }
}
