//! Small JSON file in the app's data folder that records what is installed where.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Installed {
    pub key: String,
    pub title: String,
    pub author: String,
    pub version: String,
    pub layout: String,
    pub enabled: bool,
    /// Found already in the game folder (installed by hand), not by us.
    #[serde(default)]
    pub adopted: bool,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Installed only because another mod needed it (removed again when nothing needs it).
    #[serde(default)]
    pub auto: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProfileMod {
    pub key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub version: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// A saved set of mods for one game (like a Thunderstore Mod Manager / CurseForge profile).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub mods: Vec<ProfileMod>,
    /// Set when the profile came from a modpack.
    #[serde(default)]
    pub modpack: Option<String>,
    #[serde(default)]
    pub updated: u64,
    /// Name of the Thunderstore Mod Manager / r2modman profile this was imported from.
    #[serde(default)]
    pub manager: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GameRecord {
    /// Set when the user picked the folder by hand.
    #[serde(default)]
    pub custom_path: Option<String>,
    #[serde(default)]
    pub installed: BTreeMap<String, Installed>,
    /// Cached loader package for Thunderstore games ("ts:Namespace-Name").
    #[serde(default)]
    pub base_key: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
    /// The profile whose mods are in the game folder right now.
    #[serde(default)]
    pub active_profile: Option<String>,
    /// Per-game choice for "Steam starts the normal game". None = follow the global setting.
    #[serde(default)]
    pub steam_vanilla: Option<bool>,
    /// Where plain mod files go for this game (CurseForge / files without a loader).
    #[serde(default)]
    pub mods_folder: Option<String>,
    /// Thunderstore Mod Manager profiles already imported once (deleting one doesn't bring it back).
    #[serde(default)]
    pub manager_seen: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub auto_update_mods: bool,
    pub auto_install_loader: bool,
    /// Mods only run when the game is started from ModNest; Steam starts the normal game.
    #[serde(default = "yes")]
    pub steam_vanilla: bool,
    /// Free key from console.curseforge.com; needed for CurseForge browsing.
    #[serde(default)]
    pub curseforge_key: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { auto_update_mods: true, auto_install_loader: true, steam_vanilla: true, curseforge_key: String::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Data {
    #[serde(default)]
    pub games: BTreeMap<String, GameRecord>,
    #[serde(default)]
    pub settings: Settings,
    /// Steam app id -> header image url (from Steam's store API, cached).
    #[serde(default)]
    pub art: BTreeMap<u32, String>,
    /// Game id -> tall cover image url (Thunderstore community covers).
    #[serde(default)]
    pub covers: BTreeMap<String, String>,
    /// Games added by hand in "Add a game".
    #[serde(default)]
    pub custom_games: Vec<CustomGame>,
    /// Thunderstore communities found to use MelonLoader rather than BepInEx.
    #[serde(default)]
    pub melon_games: std::collections::BTreeSet<String>,
}

/// A game added by hand: any folder, any loader.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomGame {
    pub id: String,
    pub name: String,
    pub path: String,
    /// bepinex5 | bepinex6-il2cpp | melonloader | none
    pub loader: String,
    /// Where plain mod files go (relative to the game folder), for games without a loader.
    #[serde(default)]
    pub mods_folder: String,
    /// Thunderstore community to browse for this game, if any.
    #[serde(default)]
    pub thunderstore: Option<String>,
    /// CurseForge game id to browse for this game, if any.
    #[serde(default)]
    pub curseforge: Option<u32>,
    /// The game's .exe, for starting games that aren't on Steam.
    #[serde(default)]
    pub exe: Option<String>,
    #[serde(default)]
    pub steam_app_id: Option<u32>,
}

pub struct Store {
    file: PathBuf,
    pub data: Data,
}

impl Store {
    pub fn open(dir: PathBuf) -> Store {
        let file = dir.join("modnest-state.json");
        let data = fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Store { file, data }
    }

    pub fn save(&self) {
        if let Some(d) = self.file.parent() {
            let _ = fs::create_dir_all(d);
        }
        let tmp = self.file.with_extension("json.tmp");
        if fs::write(&tmp, serde_json::to_string_pretty(&self.data).unwrap_or_default()).is_ok() {
            let _ = fs::rename(&tmp, &self.file);
        }
    }

    pub fn game(&mut self, id: &str) -> &mut GameRecord {
        self.data.games.entry(id.to_string()).or_default()
    }
}
