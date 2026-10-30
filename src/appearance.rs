use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub appearance: Appearance,
    pub text_size: f32,
    pub increase_contrast: bool,
    pub reduce_motion: bool,
    pub show_sidebar: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            text_size: 13.0,
            increase_contrast: false,
            reduce_motion: false,
            show_sidebar: true,
        }
    }
}
