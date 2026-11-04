//! Character-service lobby. Preview requests never commit a gameplay character.
use std::collections::HashMap;
use bevy::prelude::*;
use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use dsor_proto::commands::player::{CharacterGeneration, CharacterSelection, SelectionCharacter, SelectionEquipment};
use serde::Deserialize;
use crate::actors::character::{self, CharacterAnim, CharacterDesc, CharacterLibrary, ItemSkins};

pub const MAP: &str = "a0000_char";

#[derive(Clone, Debug)]
pub enum Request {
    Preview(u32), Play(u32), OpenEditor, Back,
    /// CharacterGeneration op 3: the class, gender, name and look chosen.
    Create(CharacterGeneration), Delete(u32),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Dialog {
    #[default]
    None,
    /// The creation screen (chargenwindow, crate::ui::selection).
    Create(Creation),
    /// The delete confirmation (confirmdialog) over the roster.
    Delete { id: u32, name: String },
}

/// The creation screen's data (characters/selection.json `creation`,
/// tools/export_character_selection.py).
#[derive(Deserialize, Clone, Debug, Default)]
pub struct CreationData {
    /// _Globals CharacterNameMinLength, CharacterNameMaxLength.
    pub name: [usize; 2],
    /// _Globals MaxCharactersPerAccount.
    #[serde(default)]
    pub most: usize,
    /// By wire class: 0 warrior, 1 mage, 2 ranger, 3 dwarf.
    pub classes: Vec<CreationClass>,
    /// The English gui.window.chargen_* and charselection_* texts.
    pub texts: HashMap<String, String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct CreationClass {
    /// The preview's outfit (_Template_StartEquipment.CharGenItems) and its
    /// ArmamentState (CharacterDesc::armament).
    pub items: Vec<String>,
    #[serde(default)]
    pub armament: i8,
    /// Male, female.
    pub genders: Vec<CreationLook>,
}

/// What a class and gender may look like.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct CreationLook {
    /// The wire indices of the hair and beard choices, in their order.
    pub hair: Vec<u8>,
    pub beard: Vec<u8>,
    /// The showcase: _Template_PlayerCharacter CharGenStartSequence, then
    /// CharGenIdleSequence again and again.
    pub start: String,
    pub idle: String,
}

impl CreationData {
    /// A text by its key ("gui.payment.packages_x"; "gui.window." may be left
    /// out), or the key itself.
    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
        let found = if key.starts_with("gui.") { self.texts.get(key) } else { self.texts.get(&format!("gui.window.{key}")) };
        found.map(String::as_str).unwrap_or(key)
    }
    pub fn look(&self, class: u8, gender: u8) -> Option<&CreationLook> {
        self.classes.get(class as usize)?.genders.get(gender as usize)
    }
}

/// The creation screen's choices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Creation {
    /// The name typed and the caret (a byte index into it).
    pub name: String,
    pub caret: usize,
    pub class: u8,
    pub gender: u8,
    /// The wire's body style (the ethnicity: 0 .. BODY_STYLES), hair and beard
    /// styles (choices of the class and gender's CreationLook).
    pub body: u8,
    pub hair: u8,
    pub beard: u8,
    /// The server's editor is open (op 2 came); until then nothing shows.
    pub open: bool,
    /// A request is with the server (op 1 for the editor, op 3 for the character).
    pub waiting: bool,
    /// The server made the character (op 5): the screen is gone, the world comes.
    pub done: bool,
    /// The editor's packet as the server sent it (op 2).
    pub editor: CharacterGeneration,
    /// The class description's first line shown.
    pub scroll: usize,
    /// Moves when the showcase starts again (a class or gender picked).
    pub showcase: u64,
}

/// The ethnicities a body may have: <class>_<gender>_body_00, _asian_, _black_
/// (characters/<skeleton>/outfits.json).
pub const BODY_STYLES: usize = 3;

/// A woman's beard: empty_beard. Her Beard row is hidden.
/// SOURCE: CharacterGenerationWidget::Refresh 0x1403ab8b0 (the 2026 client,
///   data/diagnostics/chargen-widget.annotated.txt).
pub const NO_BEARD: u8 = 16;

/// The hair and beard a class and gender start with (male, female).
/// SOURCE: ClientCharacterGenerationManager::ApplyDefaultHairStyle 0x140237afc,
///   called by UpdateClass 0x14023be50 and UpdateGender 0x14023c3fc.
// The Night Walker has one hair and one beard (index 0: its bald head), for
// both genders (_Template_CharacterHair/Beard NiwalkMale, NiwalkFemale).
const DEFAULT_LOOKS: [[(u8, u8); 2]; 5] = [[(18, 8), (45, 16)], [(2, 1), (3, 16)], [(7, 10), (16, 16)], [(35, 35), (33, 16)], [(0, 0), (0, 16)]];

/// The message on the screens' DenyReason line.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Notice {
    #[default]
    None,
    /// The server refused a character (CharacterGeneration op 4): its deny reason.
    Refused(u8),
    /// The server refused to play a character (CharacterSelection op 4).
    Denied(u8),
}

impl Notice {
    pub fn text(&self, data: Option<&CreationData>) -> String {
        let key = |key: &str| data.map(|d| d.text(key).to_owned()).unwrap_or_else(|| key.to_owned());
        match self {
            Notice::None => String::new(),
            // "Please enter a name with at least {0:i} letters."
            Notice::Refused(reason) => fill(&key(refusal_text(*reason)), &[&data.map_or(0, |d| d.name[0]).to_string()]),
            Notice::Denied(reason) => key(denial_text(*reason)),
        }
    }
}

/// The text of a CharacterGeneration deny reason.
/// SOURCE: SetDenyReason 0x1403abe88, its key table at 0x1409f5f34.
fn refusal_text(reason: u8) -> &'static str {
    match reason {
        1 => "chargen_existingName",
        2 => "chargen_emptyName",
        3 => "chargen_notAlphaNumeric",
        4 => "chargen_badName",
        5 => "chargen_badHairStyle",
        6 => "chargen_badBeardStyle",
        7 => "chargen_badBodyStyle",
        8 => "chargen_badBodyStature",
        9 => "chargen_alreadyGenerated",
        10 => "chargen_insufficientSlots",
        11 => "chargen_internalError",
        12 => "chargen_classLocked",
        _ => "chargen_unknownError",
    }
}

/// The text of a CharacterSelection deny reason.
/// SOURCE: ClientCharacterSelectionManager op 4 0x14023e2a0 (the 2026 client,
///   data/diagnostics/charselection-widget.annotated.txt).
fn denial_text(reason: u8) -> &'static str {
    match reason {
        1 => "charselection_badCharacter",
        2 => "charselection_badAccountData",
        3 => "charselection_badCharacterData",
        4 => "charselection_noCharacterData",
        5 => "charselection_internalError",
        _ => "charselection_unknownError",
    }
}

/// A text's {n:...} placeholders filled in order ("{0:i}", "{0:s:Name}").
pub fn fill(text: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        let Some(close) = tail.find('}') else { out.push_str(tail); return out };
        match tail[1..close].split(':').next().and_then(|n| n.parse::<usize>().ok()).and_then(|n| args.get(n)) {
            Some(arg) => out.push_str(arg),
            None => out.push_str(&tail[..=close]),
        }
        rest = &tail[close + 1..];
    }
    out.push_str(rest);
    out
}

/// A letter the name field takes: the default language's ValidCharacters (a-z,
/// A-Z, 0-9, the accented Latin letters, Greek, Cyrillic), never `<`, `>` or a
/// control character; no space, underscore or punctuation.
/// SOURCE: Util::ValidCharacters::Setup 0x1408fc7c8, TextEntry 0x14073fd4c.
pub fn name_character(c: char) -> bool {
    match c {
        '0'..='9' | 'a'..='z' | 'A'..='Z' => true,
        '\u{00C0}'..='\u{024F}' => c.is_alphabetic(),
        '\u{0386}'..='\u{03CE}' => c.is_alphabetic(),
        '\u{0400}'..='\u{045F}' => c.is_alphabetic(),
        _ => false,
    }
}

/// The highest class id: 4, the Night Walker (crate::actors::character::CLASSES).
pub const LAST_CLASS: u8 = character::CLASSES.len() as u8 - 1;

/// A class's name in keys and sound cues (charClass_warrior, ui_char_gen_choose_dwarf).
pub fn class_key(class: u8) -> &'static str {
    ["warrior", "mage", "ranger", "dwarf", "niwalk"].get(class as usize).copied().unwrap_or("warrior")
}
pub fn gender_key(gender: u8) -> &'static str {
    if gender == 1 { "female" } else { "male" }
}

/// A creation screen row with Prev/Next buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice { Gender, Body, Hair, Beard }

impl Creation {
    /// The editor as the server opened it (op 2): its class, gender and look, an
    /// empty name.
    pub fn from_editor(editor: CharacterGeneration) -> Self {
        let gender = (editor.gender.max(0) as u8).min(1);
        Self { class: (editor.player_class.max(0) as u8).min(LAST_CLASS), gender,
            body: editor.body.min(BODY_STYLES as u8 - 1), hair: editor.hair,
            beard: if gender == 1 { NO_BEARD } else { editor.beard },
            open: true, editor, ..default() }
    }

    pub fn look<'a>(&self, data: &'a CreationData) -> Option<&'a CreationLook> {
        data.look(self.class, self.gender)
    }

    /// A row's choices (wire values) and where the current one is among them.
    fn choices(&self, choice: Choice, data: &CreationData) -> (Vec<u8>, Option<usize>) {
        let (list, value): (Vec<u8>, u8) = match choice {
            Choice::Gender => (vec![0, 1], self.gender),
            Choice::Body => ((0..BODY_STYLES as u8).collect(), self.body),
            Choice::Hair => (self.look(data).map(|l| l.hair.clone()).unwrap_or_default(), self.hair),
            Choice::Beard => (self.look(data).map(|l| l.beard.clone()).unwrap_or_default(), self.beard),
        };
        let at = list.iter().position(|&v| v == value);
        (list, at)
    }

    /// How many choices a row has, and the current one's place (1-based; 0 when
    /// the current value is none of them).
    #[cfg(test)]
    pub fn place(&self, choice: Choice, data: &CreationData) -> (usize, usize) {
        let (list, at) = self.choices(choice, data);
        (list.len(), at.map_or(0, |i| i + 1))
    }

    /// Prev (-1) or Next (+1) on a row, round its choices; a value that is none
    /// of them stays (FindIndex -1). Both of Gender's arrows switch it.
    /// SOURCE: the manager's steps 0x14023c644, 0x14023c768, 0x14023b0d8,
    ///   0x14023b1fc, 0x14023b31c, 0x14023b350; HandleEvent 0x1403a8490.
    pub fn step(&mut self, choice: Choice, delta: i32, data: &CreationData) {
        if choice == Choice::Gender {
            self.set_look(self.class, 1 - self.gender.min(1));
            return;
        }
        let (list, at) = self.choices(choice, data);
        let Some(at) = at else { return };
        let next = list[(at as i32 + delta).rem_euclid(list.len() as i32) as usize];
        match choice {
            Choice::Body => self.body = next,
            Choice::Hair => self.hair = next,
            Choice::Beard => self.beard = next,
            Choice::Gender => {}
        }
    }

    /// A class button (even the class shown) or a gender: that class and
    /// gender's default hair and beard, the same ethnicity, the showcase from
    /// its start.
    pub fn set_look(&mut self, class: u8, gender: u8) {
        (self.class, self.gender) = (class.min(LAST_CLASS), gender.min(1));
        (self.hair, self.beard) = DEFAULT_LOOKS[self.class as usize][self.gender as usize];
        if self.gender == 1 { self.beard = NO_BEARD; }
        self.scroll = 0;
        self.showcase += 1;
    }

    /// Typing into the name field; false when the key changed nothing. It
    /// takes no more than CharacterNameMaxLength letters, and only the
    /// language's (name_character).
    pub fn edit(&mut self, key: KeyCode, text: Option<&str>, data: &CreationData) -> bool {
        let before = (self.name.clone(), self.caret);
        let left = self.name[..self.caret].char_indices().next_back().map_or(0, |(i, _)| i);
        let right = self.name[self.caret..].chars().next().map_or(self.caret, |c| self.caret + c.len_utf8());
        match key {
            KeyCode::ArrowLeft => self.caret = left,
            KeyCode::ArrowRight => self.caret = right,
            KeyCode::Home => self.caret = 0,
            KeyCode::End => self.caret = self.name.len(),
            KeyCode::Backspace => { self.name.replace_range(left..self.caret, ""); self.caret = left; }
            KeyCode::Delete => self.name.replace_range(self.caret..right, ""),
            _ => if let Some(text) = text {
                // An export without the creation data (name [0, 0]) takes
                // _Globals.CharacterNameMaxLength, 16.
                // FAILURE: "can't write more than 1 word ... pour le pseudo"
                //   (a characters/selection.json from before creation_data).
                let most = if data.name[1] == 0 { 16 } else { data.name[1] };
                let room = most.saturating_sub(self.name.chars().count());
                let text: String = text.chars().filter(|c| name_character(*c)).take(room).collect();
                self.name.insert_str(self.caret, &text);
                self.caret += text.len();
            }
        }
        before != (self.name.clone(), self.caret)
    }

    /// The request for this character (op 3) as the client writes it: no id, an
    /// identity matrix, the rest empty.
    /// SOURCE: SendCreate 0x140239bb0, the command's constructor 0x1409f5ac0.
    pub fn request(&self) -> CharacterGeneration {
        CharacterGeneration {
            operation: 3, player_class: self.class as i8, gender: self.gender as i8,
            hair: self.hair, beard: self.beard, body: self.body, variation: 0, name: self.name.clone(),
            matrix: std::array::from_fn(|i| if i % 5 == 0 { 1.0 } else { 0.0 }), ..default()
        }
    }
}

#[derive(Resource, Default)]
pub struct Selection {
    pub active: bool,
    pub entering: bool,
    pub offline: bool,
    pub characters: Vec<SelectionCharacter>,
    pub selected: Option<u32>,
    /// The account's character slots (CharacterSelection +0x68) and those left.
    pub capacity: u32,
    pub free: u32,
    pub first_row: usize,
    pub angle: f32,
    /// The DenyReason line's message.
    pub notice: Notice,
    pub dialog: Dialog,
    /// The creation screen's back button asked for the roster: nothing shows
    /// until it comes (op 1).
    pub returning: bool,
    /// Interface sounds to play (crate::ui::selection plays them).
    pub cues: Vec<String>,
    pub revision: u64,
    pub requests: Vec<Request>,
    pub equipment: HashMap<u32, Vec<SelectionEquipment>>,
    requested: Option<u32>,
    preview_revision: u64,
}

impl Selection {
    pub fn current(&self) -> Option<&SelectionCharacter> {
        self.characters.iter().find(|c| Some(c.character) == self.selected)
    }

    pub fn choose(&mut self, id: u32) {
        if self.entering || self.selected == Some(id) || !self.characters.iter().any(|c| c.character == id) { return; }
        self.selected = Some(id);
        self.reveal_selected();
        self.requested = None;
        self.revision += 1;
        self.preview_revision += 1;
        self.request_preview();
    }

    pub fn step(&mut self, delta: i32) {
        let Some(index) = self.characters.iter().position(|c| Some(c.character) == self.selected) else { return };
        let next = (index as i32 + delta).clamp(0, self.characters.len().saturating_sub(1) as i32) as usize;
        self.choose(self.characters[next].character);
        self.reveal_selected();
    }

    fn reveal_selected(&mut self) {
        if let Some(index) = self.characters.iter().position(|c| Some(c.character) == self.selected) {
            if index < self.first_row { self.first_row = index; }
            if index >= self.first_row + 4 { self.first_row = index - 3; }
        }
    }

    fn request_preview(&mut self) {
        if let Some(id) = self.selected {
            if !self.offline && self.requested != Some(id) {
                self.requests.push(Request::Preview(id));
                self.requested = Some(id);
            }
        }
    }

    pub fn receive(&mut self, reply: CharacterSelection, preferred: Option<u32>) {
        match reply.operation {
            // The roster (1), again after a change (7: the first character chosen).
            1 | 7 => {
                self.active = true;
                // The service repeats its initial roster. Keep the user's selection
                // and any newer preview reply instead of resetting the screen.
                // The codec's historical `free` field is release261's TOTAL
                // character_slots. `capacity` is an unrelated header field.
                let capacity = reply.free.min(100);
                let free = capacity.saturating_sub(reply.characters.len() as u32);
                let changed = self.characters != reply.characters || self.capacity != capacity || self.free != free
                    || std::mem::take(&mut self.returning) || matches!(self.dialog, Dialog::Create(_));
                self.capacity = capacity;
                self.free = free;
                self.characters = reply.characters;
                if self.characters.iter().any(|c| c.character == reply.character) {
                    self.equipment.entry(reply.character).or_insert(reply.equipment);
                }
                let valid = |id: &u32| self.characters.iter().any(|c| c.character == *id);
                let first = self.characters.first().map(|c| c.character);
                let selected = if reply.operation == 7 { first } else {
                    self.selected.filter(valid).or_else(|| preferred.filter(valid))
                        .or_else(|| Some(reply.character).filter(valid)).or(first)
                };
                if changed || self.selected != selected {
                    self.selected = selected;
                    self.entering = false;
                    self.first_row = self.first_row.min(self.characters.len().saturating_sub(4));
                    self.reveal_selected();
                    // The roster closes the creation screen (op 1 while it is open).
                    if matches!(self.dialog, Dialog::Create(_)) { self.dialog = Dialog::None; }
                    self.notice = Notice::None;
                    self.requested = None;
                    self.revision += 1;
                    self.preview_revision += 1;
                    self.equipment.retain(|id, _| self.characters.iter().any(|c| c.character == *id));
                }
                self.request_preview();
            }
            9 => {
                if !self.characters.iter().any(|c| c.character == reply.character) { return; }
                if self.equipment.get(&reply.character) != Some(&reply.equipment) {
                    self.equipment.insert(reply.character, reply.equipment);
                    if self.selected == Some(reply.character) {
                        self.preview_revision += 1;
                        self.revision += 1;
                    }
                }
            }
            4 => {
                self.entering = false;
                self.notice = Notice::Denied(reply.deny_reason);
                self.revision += 1;
            }
            _ => {}
        }
    }

    /// The roster's new-character button: the roster goes, the server is asked
    /// for its editor (op 1), whose answer (op 2) opens the creation screen.
    /// SOURCE: CharacterSelectionWidget "NewCharacter" (the 2026 layout's
    ///   CharacterAddButton) 0x14023fccc, the request 0x140239af4.
    pub fn open_editor(&mut self) {
        if self.offline || self.entering || self.dialog != Dialog::None { return; }
        self.requests.push(Request::OpenEditor);
        self.dialog = Dialog::Create(Creation { waiting: true, ..default() });
        self.revision += 1;
    }

    /// The server opened its editor (op 2): asked for, or on its own for an
    /// account without characters (and after the last one was deleted). An
    /// editor already open keeps its choices (the manager takes op 2 only
    /// before, 0x14023ab18).
    pub fn editor(&mut self, reply: CharacterGeneration) {
        if matches!(&self.dialog, Dialog::Create(c) if c.open && !c.done) { return; }
        self.active = true;
        self.entering = false;
        self.returning = false;
        self.dialog = Dialog::Create(Creation::from_editor(reply));
        self.angle = 0.0;
        self.notice = Notice::None;
        self.revision += 1;
        self.preview_revision += 1;
    }

    /// The server refused the character (op 4): its deny reason's message.
    pub fn creation_refused(&mut self, reason: u8) {
        let Dialog::Create(creation) = &mut self.dialog else { return };
        if !creation.open { return; }
        creation.waiting = false;
        self.notice = Notice::Refused(reason);
        self.revision += 1;
    }

    /// The server made the character (op 5): the screen goes, the world comes.
    pub fn created(&mut self) {
        let Dialog::Create(creation) = &mut self.dialog else { return };
        if !creation.waiting { return; }
        creation.done = true;
        self.cues.push(format!("ui/char_gen/ui_char_gen_generate_{}_{}", class_key(creation.class), gender_key(creation.gender)));
        self.entering = true;
        self.revision += 1;
    }

    /// The creation screen's back button: the roster again (op 2 asks the
    /// server for it; an account without characters gets the editor back).
    /// SOURCE: HandleEvent "CharacterSelection" 0x1403a8490, 0x14023f8b4; the
    ///   button is off when the editor's first bool says so (Load 0x1403a971c).
    pub fn leave_editor(&mut self) {
        if !matches!(&self.dialog, Dialog::Create(c) if c.open && !c.waiting && !c.done && !c.editor.flag) { return; }
        self.requests.push(Request::Back);
        self.dialog = Dialog::None;
        self.returning = true;
        self.notice = Notice::None;
        self.revision += 1;
        self.preview_revision += 1;
    }

    /// The creation screen's choices changed: the screen and the preview follow
    /// (a new class or gender also turns the preview to the front).
    pub fn creation_changed(&mut self, look: bool, turn: bool) {
        self.revision += 1;
        if look { self.preview_revision += 1; }
        if turn { self.angle = 0.0; }
    }

    /// The creation screen's Create button: the character asked for (op 3), once.
    /// The client checks nothing more: the server answers op 4 or 5.
    /// SOURCE: SendCreate 0x140239bb0 (only in the editor's state 2).
    pub fn create(&mut self) {
        let Dialog::Create(creation) = &mut self.dialog else { return };
        if !creation.open || creation.waiting || creation.done { return; }
        creation.waiting = true;
        self.requests.push(Request::Create(creation.request()));
        self.revision += 1;
    }

    /// The roster's delete button: the confirmation.
    pub fn ask_delete(&mut self) {
        if self.entering || self.dialog != Dialog::None { return; }
        let Some(c) = self.current() else { return };
        self.dialog = Dialog::Delete { id: c.character, name: c.name.clone() };
        self.revision += 1;
    }

    /// The confirmation's answer. Confirmed, the character is deleted (op 6) and
    /// leaves the roster at once, the first one chosen; the server's next
    /// roster refreshes the screen.
    /// SOURCE: DeleteCharacter 0x14023f570.
    pub fn answer_delete(&mut self, confirmed: bool) {
        let Dialog::Delete { id, .. } = self.dialog else { return };
        self.dialog = Dialog::None;
        if confirmed {
            self.requests.push(Request::Delete(id));
            self.characters.retain(|c| c.character != id);
            self.equipment.remove(&id);
            self.free = self.capacity.saturating_sub(self.characters.len() as u32);
            self.selected = None;
            self.first_row = 0;
            if let Some(first) = self.characters.first().map(|c| c.character) { self.choose(first); }
            self.preview_revision += 1;
        }
        self.revision += 1;
    }

    pub fn play(&mut self) {
        if self.offline || self.entering || self.dialog != Dialog::None { return; }
        if let Some((id, class, gender)) = self.current().map(|c| (c.character, c.player_class as u8, c.gender as u8)) {
            self.cues.push(format!("ui/char_gen/ui_char_gen_generate_{}_{}", class_key(class), gender_key(gender)));
            self.requests.push(Request::Play(id));
            self.entering = true;
            self.revision += 1;
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct SelectionCamera {
    pub pitch: f32, pub yaw: f32, pub offset: [f32; 3], pub fov: f32,
    #[serde(default)] pub matrix: Option<[f32; 16]>,
    #[serde(default = "camera_near")] pub near: f32,
    #[serde(default = "camera_far")] pub far: f32,
}
fn camera_near() -> f32 { 0.1 }
fn camera_far() -> f32 { 1000. }
impl SelectionCamera {
    pub fn transform(&self) -> Transform {
        if let Some(matrix) = self.matrix { return Transform::from_matrix(Mat4::from_cols_array(&matrix)); }
        let rotation = Quat::from_euler(EulerRot::YXZ, self.yaw.to_radians(), self.pitch.to_radians(), 0.);
        Transform::from_translation(Vec3::new(self.offset[0], self.offset[1], -self.offset[2])).with_rotation(rotation)
    }
    pub fn lens(&self, aspect: f32) -> crate::rendering::camera_view::Lens {
        crate::rendering::camera_view::Lens { fov: self.fov, aspect, near: self.near, far: self.far }
    }
}

#[derive(Asset, TypePath, Deserialize)]
pub struct SelectionData {
    pub camera: SelectionCamera,
    pub slots: HashMap<String, Vec<u8>>,
    pub hair: HashMap<String, HashMap<String, String>>,
    pub beard: HashMap<String, HashMap<String, String>>,
    /// The creation screen's (older exports have none: the screen stays empty).
    #[serde(default)]
    pub creation: CreationData,
}

#[derive(Default, TypePath)]
struct SelectionLoader;
impl AssetLoader for SelectionLoader {
    type Asset = SelectionData;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new(); r.read_to_end(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] { &["selection.json"] }
}

#[derive(Resource)]
struct Scene {
    data: Handle<SelectionData>, shown: Option<Entity>, pending: Option<Entity>, revision: Option<u64>,
    /// The creation preview's showcase shown (Creation::showcase).
    showcase: Option<u64>,
}

/// The creation preview's showcase: CharSelectStart with the class and gender's
/// start sequence, then CharSelectIdle with its idle sequence again and again.
#[derive(Component)]
struct Showcase { start: String, idle: String, idling: bool, started: bool }

#[derive(Component)]
pub struct Preview;

pub struct SelectionPlugin;
/// DSOR_CREATE=<class>,<gender>,<name>: on the roster, opens the creation
/// screen and creates that character, as the buttons would (testing a class
/// end to end without clicking: `--user ... DSOR_CREATE=4,0,Nightwalker`).
/// DSOR_PLAY=1: then enters the world with the selected character (`--char`),
/// as Play Now would.
fn auto_create(mut selection: ResMut<Selection>, mut done: Local<bool>, mut played: Local<bool>) {
    if !selection.active || selection.offline || selection.entering { return; }
    if *done && !*played && std::env::var("DSOR_PLAY").is_ok() && selection.dialog == Dialog::None && selection.selected.is_some() {
        selection.play();
        *played = true;
        return;
    }
    if *done { return; }
    let Some((class, gender, name)) = std::env::var("DSOR_CREATE").ok().and_then(|v| {
        let mut it = v.splitn(3, ',');
        Some((it.next()?.trim().parse::<u8>().ok()?, it.next()?.trim().parse::<u8>().ok()?, it.next()?.trim().to_owned()))
    }) else { *done = true; return };
    match &mut selection.dialog {
        Dialog::None => selection.open_editor(),
        Dialog::Create(c) if c.open && !c.waiting => {
            c.set_look(class, gender);
            c.name = name;
            c.caret = c.name.len();
            selection.create();
            *done = true;
        }
        _ => {}
    }
}

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>().init_asset::<SelectionData>().init_asset_loader::<SelectionLoader>()
            .add_systems(Startup, setup)
            .add_systems(Update, (scene, showcase).chain().after(crate::network::net::NetReceive).before(crate::network::net::NetSystems));
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Update, replay.after(scene));
        app.add_systems(Update, auto_create.after(scene));
    }
}

/// Opt-in native renderer regression, matching the equipment-replay facility.
/// It consumes explicit actions from a local fixture; ordinary sessions never
/// create this resource or send any automatic selection request.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Deserialize)]
struct ReplayStep {
    frame: u32,
    select: Option<u32>,
    rotate: Option<f32>,
    capture: Option<String>,
    #[serde(default)] play: bool,
    #[serde(default)] quit: bool,
    /// The creation screen: a class button, a row's arrow ("gender", "body",
    /// "hair" or "beard", -1 or +1), the name typed, Create, the server's
    /// refusal (deny reason), the back button.
    class: Option<u8>,
    step: Option<(String, i32)>,
    name: Option<String>,
    #[serde(default)] create: bool,
    refuse: Option<u8>,
    #[serde(default)] back: bool,
    /// The roster's delete button, the confirmation's answer.
    #[serde(default)] delete: bool,
    confirm: Option<bool>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Replay { loaded: bool, frame: u32, steps: std::collections::VecDeque<ReplayStep>, quit_at: Option<u32> }

#[cfg(not(target_arch = "wasm32"))]
fn replay(mut commands: Commands, mut state: Local<Replay>, mut selection: ResMut<Selection>, scene: Res<Scene>,
    loading: Res<crate::screens::loading::LoadingScreen>, current: Option<Res<crate::world::map::CurrentMap>>,
    data: Res<Assets<SelectionData>>) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    if !state.loaded {
        state.loaded = true;
        if let Ok(path) = std::env::var("DSOR_SELECTION_REPLAY") {
            match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
                Ok(steps) => state.steps = steps,
                Err(e) => error!("character selection replay: {e}"),
            }
        }
    }
    if state.steps.is_empty() && state.quit_at.is_none() { return; }
    if loading.active() || !current.is_some_and(|m| m.spawned) || (selection.active &&
        (scene.pending.is_some() || scene.revision != Some(selection.preview_revision))) { return; }
    state.frame += 1;
    if state.quit_at.is_some_and(|at| state.frame >= at) { commands.write_message(AppExit::Success); return; }
    while state.steps.front().is_some_and(|s| s.frame <= state.frame) {
        let step = state.steps.pop_front().unwrap();
        if let Some(id) = step.select { selection.choose(id); }
        if let Some(angle) = step.rotate { selection.angle = angle; }
        if step.play { selection.play(); }
        let creation = data.get(&scene.data).map(|d| d.creation.clone()).unwrap_or_default();
        if let Dialog::Create(c) = &selection.dialog {
            let mut c = c.clone();
            let before = c.clone();
            if let Some(class) = step.class { c.set_look(class, c.gender); }
            if let Some((row, delta)) = &step.step {
                let choice = match row.as_str() { "gender" => Choice::Gender, "body" => Choice::Body, "hair" => Choice::Hair, _ => Choice::Beard };
                c.step(choice, *delta, &creation);
            }
            if let Some(name) = &step.name { c.name.clone_from(name); c.caret = name.len(); }
            if c != before {
                let turn = c.showcase != before.showcase;
                selection.dialog = Dialog::Create(c);
                selection.creation_changed(true, turn);
            }
        }
        if step.create { selection.create(); }
        if let Some(reason) = step.refuse { selection.creation_refused(reason); }
        if step.back { selection.leave_editor(); }
        if step.delete { selection.ask_delete(); }
        if let Some(confirmed) = step.confirm { selection.answer_delete(confirmed); }
        if let Some(path) = step.capture { commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path)); }
        if step.quit { state.quit_at = Some(state.frame + 30); }
        info!("character selection replay frame {}", state.frame);
    }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>, mut selection: ResMut<Selection>) {
    commands.insert_resource(Scene { data: assets.load("characters/selection.json"), shown: None, pending: None, revision: None, showcase: None });
    // Deterministic server-packet fixture, without authenticating or changing an account.
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(path) = std::env::var("DSOR_SELECTION_PREVIEW") {
        let result = (|| -> Option<()> {
            let packets: Vec<(String, usize)> = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
            selection.offline = true;
            for (hex, bits) in packets {
                let bytes: Vec<u8> = hex.as_bytes().chunks_exact(2).map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap_or(""), 16)).collect::<Result<_, _>>().ok()?;
                // The server's roster replies and its creation editor's (op 2 opens it,
                // op 4 refuses a character).
                if let Some(c) = dsor_proto::messages::character_generation(&bytes, bits) {
                    match c.operation {
                        2 => selection.editor(c),
                        4 => selection.creation_refused(c.deny_reason),
                        _ => {}
                    }
                    continue;
                }
                selection.receive(dsor_proto::messages::character_selection(&bytes, bits)?, None);
            }
            commands.insert_resource(crate::world::map::CurrentMap::new(MAP.into(), assets.load(format!("maps/{MAP}.map.json"))));
            Some(())
        })();
        if result.is_none() { error!("Invalid character-selection preview fixture"); }
    }
}

/// The creation screen's character: its choices in the class's preview outfit,
/// with the class's armament.
/// SOURCE: ClientCharacterGenerationManager::UpdateCharacterAppearance 0x14023b380.
pub fn creation_description(c: &Creation, data: &SelectionData, skins: &ItemSkins) -> CharacterDesc {
    let class = data.creation.classes.get(c.class as usize);
    let items: Vec<_> = class.map(|k| k.items.iter()
        .map(|template| SelectionEquipment { template: template.clone(), ..default() }).collect()).unwrap_or_default();
    let character = SelectionCharacter { player_class: c.class as u32, gender: c.gender as u32,
        hair: c.hair, beard: c.beard, body: c.body, ..default() };
    let mut desc = description(&character, &items, data, skins);
    if let Some(armament) = class.map(|k| k.armament).filter(|a| *a > 0) { desc.armament = armament; }
    desc
}

pub fn description(c: &SelectionCharacter, equipment: &[SelectionEquipment], data: &SelectionData, skins: &ItemSkins) -> CharacterDesc {
    let mut worn = Vec::new();
    for e in equipment {
        let parts = skins.appearances.get(&e.slot).or_else(|| skins.items.get(&e.template)).cloned().unwrap_or_default();
        // The wire list has no slot number, so resolve SlotType from the static
        // item table. Its order cannot identify a slot when others are empty.
        for &slot in data.slots.get(&e.template).into_iter().flatten() {
            worn.push(crate::actors::appearance::Equipment { slot, skin_parts: parts.clone(), colour: (e.colour[3] > 0.0).then_some(e.colour) });
        }
    }
    worn = crate::actors::appearance::normalize(worn);
    worn.dedup_by(|a, b| a.slot == b.slot);
    let mut desc = CharacterDesc { class: c.player_class.min(LAST_CLASS as u32) as u8, gender: c.gender.min(1) as u8,
        armament: crate::actors::appearance::remote_armament(&worn, ""), equipment: worn,
        body: c.body, stature: c.variation, ..default() };
    // The selection screens give a woman no beard (UpdateCharacterAppearance
    // 0x140240270 and 0x14023b380; the world does, NewPlayer 0x14020f828).
    let beard = (desc.gender != 1).then_some(c.beard);
    desc.cosmetics = cosmetics(data, desc.class, desc.gender, Some(c.hair), beard);
    desc
}

/// The hair and beard parts of a class and gender's styles (wire indices).
pub fn cosmetics(data: &SelectionData, class: u8, gender: u8, hair: Option<u8>, beard: Option<u8>) -> Vec<String> {
    let key = format!("{}_{}", character::CLASSES.get(class as usize).copied().unwrap_or("warrior"), if gender == 1 { "female" } else { "male" });
    [(&data.hair, hair), (&data.beard, beard)].into_iter()
        .filter_map(|(table, index)| table.get(&key)?.get(&index?.to_string()).cloned()).collect()
}

#[allow(clippy::too_many_arguments)]
fn scene(mut commands: Commands, selection: Res<Selection>, mut scene: ResMut<Scene>,
    data: Res<Assets<SelectionData>>, skins: Res<Assets<ItemSkins>>, library: Res<CharacterLibrary>,
    mut previews: Query<(&CharacterAnim, &character::Character, &mut Visibility, &mut Transform), With<Preview>>,
    mut cameras: Query<(&mut Transform, &mut Projection), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>, Without<Preview>)>) {
    if !selection.active {
        for id in [scene.shown.take(), scene.pending.take()].into_iter().flatten() { commands.entity(id).try_despawn(); }
        scene.revision = None;
        return;
    }
    let Some(data) = data.get(&scene.data) else { return };
    let cam = &data.camera;
    for (mut tf, mut projection) in &mut cameras {
        // Nebula stores the camera's offset along forward (-Z); Bevy stores
        // the camera position behind the portrait, looking down -Z.
        *tf = cam.transform();
        let aspect = if let Projection::Perspective(p) = &*projection { p.aspect_ratio } else { 16. / 9. };
        *projection = cam.lens(aspect).projection();
    }
    let creating = match &selection.dialog { Dialog::Create(c) => Some(c), _ => None };
    if scene.revision != Some(selection.preview_revision) {
        let skins = skins.get(&library.item_skins);
        if let Some(c) = creating {
            // Until the server's editor opens, the roster's character stays.
            if !c.open {
                scene.revision = Some(selection.preview_revision);
            } else if let Some(skins) = skins {
                if let Some(old) = scene.pending.take() { commands.entity(old).try_despawn(); }
                let entity = character::spawn_character(&mut commands, creation_description(c, data, skins), Transform::default());
                // Another class or gender plays its showcase from the start; another
                // hair, beard or ethnicity carries on standing.
                let restart = scene.showcase != Some(c.showcase);
                let look = data.creation.look(c.class, c.gender).cloned().unwrap_or_default();
                let mut anim = CharacterAnim::default();
                anim.state = character::AnimState::Named(if restart { "CharSelectStart" } else { "CharSelectIdle" }.into());
                commands.entity(entity).insert((Preview, anim, Visibility::Hidden,
                    Showcase { start: look.start, idle: look.idle, idling: !restart, started: false }));
                scene.pending = Some(entity);
                scene.revision = Some(selection.preview_revision);
                scene.showcase = Some(c.showcase);
            }
        } else if let (Some(c), Some(skins)) = (selection.current(), skins) {
            if let Some(equipment) = selection.equipment.get(&c.character) {
                if let Some(old) = scene.pending.take() { commands.entity(old).try_despawn(); }
                let entity = character::spawn_character(&mut commands, description(c, equipment, data, skins), Transform::default());
                let mut anim = CharacterAnim::default();
                anim.state = character::AnimState::Named("CharSelectIdle".into());
                commands.entity(entity).insert((Preview, anim, Visibility::Hidden));
                scene.pending = Some(entity);
                scene.revision = Some(selection.preview_revision);
                scene.showcase = None;
                info!("character preview: {} ({})", c.name, c.character);
            }
        } else if selection.current().is_none() {
            for id in [scene.shown.take(), scene.pending.take()].into_iter().flatten() { commands.entity(id).try_despawn(); }
        }
    }
    if let Some(id) = scene.pending {
        if let Ok((anim, character, mut visibility, _)) = previews.get_mut(id) {
            if anim.is_ready() && character.outfit_ready() {
                *visibility = Visibility::Inherited;
                if let Some(old) = scene.shown.replace(id) { commands.entity(old).try_despawn(); }
                scene.pending = None;
            }
        }
    }
    for (_, _, _, mut tf) in &mut previews { tf.rotation = Quat::from_rotation_y(selection.angle); }
}

/// The shown creation preview's showcase: when it shows, a class or gender's
/// start sequence (again and again if it repeats: the mage's ball) and its idle
/// sequence looping; CharSelectStart, then CharSelectIdle.
/// SOURCE: RestartCharGenSequences 0x140140458 (charactergeneration/<CharGen
///   StartSequence> only with restartAll, <CharGenIdleSequence> always,
///   looping); UpdateCharacterAppearance 0x14023b380 (start and loop animation
///   states). The sequences hold sounds, the mage's charscreen_mage_ball on its
///   left hand and its light.
fn showcase(mut commands: Commands, mut previews: Query<(Entity, &mut CharacterAnim, &InheritedVisibility, &mut Showcase), With<Preview>>,
    players: Query<&AnimationPlayer>, skills: Option<Res<crate::gameplay::skills::SkillData>>,
    tables: Res<Assets<crate::gameplay::skills::SequenceTable>>) {
    let table = skills.as_ref().and_then(|s| tables.get(s.sequences()));
    for (entity, mut anim, shown, mut show) in &mut previews {
        if !shown.get() { continue; }
        if !show.started {
            let Some(table) = table else { continue };
            show.started = true;
            let mut play = |name: &str, looping: bool| if let Some(seq) = table.0.get(name).cloned() {
                info!("character creation showcase: {name}");
                let looping = looping || seq.repeat;
                crate::gameplay::skills::play_with(&mut commands, seq, Some(entity), entity, looping, true);
            };
            if !show.idling { play(&show.start, false); }
            play(&show.idle, true);
        }
        if !show.idling && anim.playback_finished(&players) == Some(true) {
            show.idling = true;
            anim.state = character::AnimState::Named("CharSelectIdle".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn roster() -> CharacterSelection {
        CharacterSelection { operation: 1, character: 1, capacity: 200_000_003, free: 5,
            characters: vec![SelectionCharacter { character: 1, name: "One".into(), ..default() },
                SelectionCharacter { character: 2, name: "Two".into(), ..default() }], ..default() }
    }
    #[test]
    fn browsing_and_duplicate_rosters_never_play_or_overwrite_a_newer_preview() {
        let mut state = Selection::default(); state.receive(roster(), None);
        assert_eq!((state.capacity, state.free), (5, 3));
        assert!(matches!(state.requests.as_slice(), [Request::Preview(1)]));
        state.requests.clear(); state.choose(2);
        let rev = state.preview_revision;
        state.receive(roster(), None);
        assert_eq!(state.selected, Some(2)); assert_eq!(state.preview_revision, rev);
        state.receive(CharacterSelection { operation: 9, character: 1, equipment: vec![SelectionEquipment { template: "late".into(), ..default() }], ..default() }, None);
        assert_eq!(state.preview_revision, rev, "late reply must not replace the chosen character");
        assert!(state.requests.iter().all(|r| matches!(r, Request::Preview(_))));
        state.requests.clear(); state.play(); state.play();
        assert!(matches!(state.requests.as_slice(), [Request::Play(2)]));
    }
    #[test]
    fn roster_refresh_and_rejected_creation_remain_interactive() {
        let mut state = Selection::default(); state.receive(roster(), Some(2));
        assert_eq!(state.selected, Some(2));
        let mut smaller = roster(); smaller.characters.pop(); smaller.free = 4;
        state.receive(smaller, None); assert_eq!(state.selected, Some(1));
        state.requests.clear();
        state.open_editor();
        assert!(matches!(state.requests.as_slice(), [Request::OpenEditor]));
        assert!(matches!(&state.dialog, Dialog::Create(c) if c.waiting && !c.open));
        state.editor(CharacterGeneration { operation: 2, player_class: 3, gender: 1, hair: 31, beard: 16, ..default() });
        let Dialog::Create(c) = &mut state.dialog else { panic!() };
        assert!(c.open && !c.waiting);
        assert_eq!((c.class, c.gender, c.hair, c.beard), (3, 1, 31, 16));
        c.name = "Coppergear".into();
        c.waiting = true;
        state.creation_refused(1);
        assert!(matches!(&state.dialog, Dialog::Create(c) if !c.waiting && c.name == "Coppergear"));
        assert_eq!(state.notice, Notice::Refused(1));
        assert_eq!(state.notice.text(Some(&data())), "The selected name already exists!");
    }

    #[test]
    fn a_name_types_past_one_letter_without_the_creation_data() {
        let mut c = Creation::default();
        let empty = CreationData::default();
        for ch in "Thorgal".chars() { c.edit(KeyCode::KeyA, Some(&ch.to_string()), &empty); }
        assert_eq!(c.name, "Thorgal");
    }
    fn data() -> CreationData {
        let look = |hair: Vec<u8>, beard: Vec<u8>| CreationLook { hair, beard, start: "start".into(), idle: "idle".into() };
        CreationData { name: [3, 16], most: 5, classes: (0..4).map(|_| CreationClass { items: vec!["CharCreation_x".into()], armament: 5,
            genders: vec![look(vec![2, 7, 17, 18, 19, 35], vec![1, 8, 9, 10, 35]), look(vec![3, 16, 21, 33, 45], vec![16])] }).collect(),
            texts: [("gui.window.chargen_existingName", "The selected name already exists!"),
                ("gui.window.chargen_emptyName", "Please enter a name with at least {0:i} letters."),
                ("gui.window.chargen_notAlphaNumeric", "The name may only include A-Z, a-z, and 0-9."),
                ("gui.window.chargen_unknownError", "Unknown error."),
                ("gui.window.charselection_badCharacter", "Wrong character selected")]
                .into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect() }
    }

    #[test]
    fn creation_choices_go_round_and_a_class_or_gender_starts_over_with_its_defaults() {
        let data = data();
        let mut c = Creation::from_editor(CharacterGeneration { operation: 2, hair: 18, beard: 8, ..default() });
        assert_eq!(c.place(Choice::Hair, &data), (6, 4));
        c.step(Choice::Hair, 1, &data); assert_eq!(c.hair, 19);
        c.step(Choice::Hair, 1, &data); c.step(Choice::Hair, 1, &data); assert_eq!(c.hair, 2, "round");
        c.step(Choice::Hair, -1, &data); assert_eq!(c.hair, 35);
        c.step(Choice::Body, -1, &data); assert_eq!(c.body, 2);
        let showcase = c.showcase;
        // Both arrows switch the gender: her class's defaults, no beard, the same ethnicity.
        c.step(Choice::Gender, -1, &data);
        assert_eq!((c.gender, c.hair, c.beard, c.body), (1, 45, NO_BEARD, 2));
        assert_eq!(c.place(Choice::Beard, &data), (1, 1));
        assert_eq!(c.showcase, showcase + 1);
        c.step(Choice::Gender, 1, &data); assert_eq!((c.gender, c.hair, c.beard), (0, 18, 8));
        c.set_look(1, 0); assert_eq!((c.class, c.hair, c.beard, c.showcase), (1, 2, 1, showcase + 3));
        c.hair = 7;
        c.set_look(1, 0);
        assert_eq!((c.hair, c.showcase), (2, showcase + 4), "the class shown, clicked again, starts over too");
        c.hair = 99;
        c.step(Choice::Hair, 1, &data); assert_eq!(c.hair, 99, "a value none of the choices stays");
    }

    #[test]
    fn the_name_takes_only_letters_and_digits_and_the_request_carries_the_look() {
        let data = data();
        let mut state = Selection::default();
        state.editor(CharacterGeneration { operation: 2, slot: 7, hair: 18, beard: 8, words: [0, 0, 0, 1362], flag: false, ..default() });
        let Dialog::Create(c) = &mut state.dialog else { panic!() };
        for text in ["A", " ", "_", "<", "b", "\u{7}", "é", "Ж"] { c.edit(KeyCode::KeyA, Some(text), &data); }
        assert_eq!(c.name, "AbéЖ");
        c.edit(KeyCode::ArrowLeft, None, &data); c.edit(KeyCode::Backspace, None, &data);
        assert_eq!(c.name, "AbЖ");
        for _ in 0..20 { c.edit(KeyCode::KeyX, Some("x"), &data); }
        assert_eq!(c.name.chars().count(), 16, "no more than CharacterNameMaxLength");
        c.name = "Ab".into();
        c.step(Choice::Body, 1, &data);
        // The client sends even a short name: the server answers.
        state.create();
        let [Request::Create(r)] = state.requests.as_slice() else { panic!("{:?}", state.requests) };
        assert_eq!((r.operation, r.player_class, r.gender, r.hair, r.beard, r.body, r.variation), (3, 0, 0, 18, 8, 1, 0));
        assert_eq!((r.character, r.slot, r.words, r.flag, r.request_flag, r.text.as_str()), (0, 0, [0; 4], false, false, ""));
        assert_eq!((r.matrix[0], r.matrix[5], r.matrix[10], r.matrix[15], r.matrix[1]), (1.0, 1.0, 1.0, 1.0, 0.0));
        assert_eq!(r.name, "Ab");
        state.requests.clear();
        state.create();
        assert!(state.requests.is_empty(), "one request at a time");
        state.leave_editor();
        assert!(state.requests.is_empty(), "no way back while the character is being made");
        state.creation_refused(2);
        assert_eq!(state.notice.text(Some(&data)), "Please enter a name with at least 3 letters.");
        state.creation_refused(13);
        assert_eq!(state.notice.text(Some(&data)), "Unknown error.");
        state.create();
        state.created();
        assert!(matches!(&state.dialog, Dialog::Create(c) if c.done) && state.entering);
        assert_eq!(state.cues, ["ui/char_gen/ui_char_gen_generate_warrior_male"]);
    }

    #[test]
    fn the_back_button_returns_through_the_server_and_an_open_editor_ignores_another() {
        let data = data();
        let mut state = Selection::default();
        state.receive(roster(), None);
        state.open_editor();
        state.editor(CharacterGeneration { operation: 2, hair: 18, beard: 8, ..default() });
        let Dialog::Create(c) = &mut state.dialog else { panic!() };
        c.set_look(3, 0);
        state.editor(CharacterGeneration { operation: 2, hair: 18, beard: 8, ..default() });
        assert!(matches!(&state.dialog, Dialog::Create(c) if c.class == 3), "op 2 again keeps the choices");
        state.requests.clear();
        state.leave_editor();
        assert!(matches!(state.requests.as_slice(), [Request::Back]));
        assert!(state.returning && state.dialog == Dialog::None);
        state.receive(roster(), None);
        assert!(!state.returning, "the roster comes back on the server's op 1");
        // An editor whose first bool is set has no way back.
        state.editor(CharacterGeneration { operation: 2, flag: true, ..default() });
        state.requests.clear();
        state.leave_editor();
        assert!(state.requests.is_empty());
        // A refused entry into the world says why.
        let mut state = Selection::default();
        state.receive(roster(), None);
        state.receive(CharacterSelection { operation: 4, deny_reason: 1, ..default() }, None);
        assert_eq!(state.notice.text(Some(&data)), "Wrong character selected");
    }

    #[test]
    fn placeholders_fill_in_order() {
        assert_eq!(fill("at least {0:i} letters", &["3"]), "at least 3 letters");
        assert_eq!(fill("{0:i:AmountOfBookings} orders, {1:s}", &["2", "x"]), "2 orders, x");
        assert_eq!(fill("keep {2:s}", &["a"]), "keep {2:s}");
    }
    #[test]
    fn preselected_character_and_preview_changes_stay_in_the_visible_rows() {
        let mut reply = roster();
        reply.characters = (1..=5).map(|character| SelectionCharacter { character, ..default() }).collect();
        let mut state = Selection::default(); state.receive(reply, Some(5));
        assert_eq!(state.selected, Some(5)); assert_eq!(state.first_row, 1);
        state.choose(1); assert_eq!(state.first_row, 0);
        state.choose(5); assert_eq!(state.first_row, 1);
    }
    #[test]
    fn preview_resolves_sparse_slots_transmog_dyes_and_two_handed_duplicates() {
        let data: SelectionData = serde_json::from_str(r#"{"camera":{"pitch":-3,"yaw":0,"offset":[0,2,-14.5],"fov":20},"slots":{"torso":[2],"staff":[5,6]},"hair":{"mage_female":{"3":"hair"}},"beard":{}}"#).unwrap();
        let skins: ItemSkins = serde_json::from_str(r#"{"items":{"torso":["coat"],"staff":["staff"]},"appearances":{"skin":["transmog"]}}"#).unwrap();
        let staff = SelectionEquipment { template: "staff".into(), ..default() };
        let desc = description(&SelectionCharacter { player_class: 1, gender: 1, hair: 3, ..default() },
            &[SelectionEquipment { template: "torso".into(), slot: "skin".into(), colour: [1.0, 0.0, 0.0, 1.0], ..default() }, staff.clone(), staff], &data, &skins);
        assert_eq!(desc.equipment.len(), 3); assert_eq!(desc.equipment[0].slot, 2);
        assert_eq!(desc.equipment[0].skin_parts, ["transmog"]);
        assert_eq!(desc.equipment[0].colour, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(desc.armament, 5); assert_eq!(desc.cosmetics, ["hair"]);
    }
}
