//! The commands the client sends: `[u8 0x8B][u16 id][body]`, no actor, no terminator.
//!
//! SOURCE: the experimental server's decoders of client commands (server.py and the
//!   dsor/*.py `decode_*` / `parse_*` functions), checked byte for byte against the
//!   captured client payloads in tests/commands_client.rs.

use dsor_raknet::{BitReader, BitWriter};

use super::wire::{Body, Raw};
use super::{actors, combat, inventory, movement, player, social, DecodeError, CLIENT, PADDING_BITS};

macro_rules! client_commands {
    ($($id:literal => $var:ident($ty:ty),)*) => {
        /// Every command the client sends, as the server reads it.
        #[derive(Debug, Clone, PartialEq)]
        pub enum ClientCommand {
            $($var($ty),)*
            /// A command with no layout here: the id and the body verbatim.
            Unknown { id: u16, body: Raw },
        }

        impl ClientCommand {
            pub fn id(&self) -> u16 {
                match self {
                    $(Self::$var(_) => $id,)*
                    Self::Unknown { id, .. } => *id,
                }
            }

            pub fn is_known(id: u16) -> bool {
                matches!(id, $($id)|*)
            }

            fn decode_body(id: u16, r: &mut BitReader<'_>) -> Option<Result<Self, DecodeError>> {
                Some(match id {
                    $($id => <$ty as Body>::decode(r).map(Self::$var),)*
                    _ => return None,
                })
            }

            fn encode_body(&self, w: &mut BitWriter) {
                match self {
                    $(Self::$var(c) => c.encode(w),)*
                    Self::Unknown { body, .. } => body.write(w),
                }
            }
        }
    };
}

client_commands! {
    // player.rs
    34 => ActorRequest(player::ActorRequest),
    41 => UpdateShowPlayerOverheadIcons(player::UpdateShowPlayerOverheadIcons),
    43 => SettingsInfo(player::SettingsInfo),
    104 => UnlockMap(player::UnlockMap),
    105 => Travel(player::Travel),
    107 => Logout(player::Logout),
    120 => StillAlive(player::StillAlive),
    143 => CharacterGeneration(player::CharacterGeneration),
    144 => CharacterSelection(player::CharacterSelection),
    189 => PlayerQuery(player::PlayerQuery),
    196 => GuildMessageOfTheDayRequest(player::GuildMessageOfTheDayRequest),
    237 => ChatClientLoggedIn(player::ChatClientLoggedIn),
    238 => ChatErrorReport(player::ChatErrorReport),
    243 => ClientAverageFrameInfoTracking(player::ClientAverageFrameInfoTracking),
    244 => ClientCurrentPositionFrameInfoTracking(player::ClientCurrentPositionFrameInfoTracking),
    245 => ClientLogEvent(player::ClientLogEvent),
    257 => OverallUserRankingInfo(player::OverallUserRankingInfo),
    259 => OverallLocalUserRankingInfo(player::OverallLocalUserRankingInfo),
    282 => StagingSignal(player::StagingSignal),
    305 => GauntletNotificationInfo(player::GauntletNotificationInfo),
    // actors.rs
    47 => NpcRequest(actors::NpcRequest),
    62 => PropInteraction(actors::PropInteraction),
    108 => PickupItem(actors::PickupItem),
    110 => Encounter(actors::Encounter),
    111 => Talk(actors::Talk),
    112 => Deliver(actors::Deliver),
    113 => Pay(actors::Pay),
    147 => Quest(actors::Quest),
    150 => QuestMonsterIndicationRequest(actors::QuestMonsterIndicationRequest),
    152 => QuestTriggerIndicationRequest(actors::QuestTriggerIndicationRequest),
    267 => DebugMonsterRequest(actors::DebugMonsterRequest),
    299 => QuestTriggerSignal(actors::QuestTriggerSignal),
    // movement.rs
    103 => Move(movement::Move),
    // combat.rs
    73 => Skill(combat::Skill),
    74 => TargetSkill(combat::TargetSkill),
    75 => BulletSkill(combat::BulletSkill),
    76 => TargetPointBulletSkill(combat::TargetPointBulletSkill),
    77 => ShiftedSkill(combat::ShiftedSkill),
    79 => SustainedSkill(combat::ClientSustainedSkill),
    106 => Respawn(combat::Respawn),
    119 => Resurrect(combat::Resurrect),
    297 => QuickResurrectGroupMember(combat::QuickResurrectGroupMember),
    // inventory.rs
    84 => QuickSlots(inventory::QuickSlots),
    88 => Inventory(inventory::Inventory),
    90 => CurrencyConversion(inventory::CurrencyConversion),
    92 => RequestOffer(inventory::RequestOffer),
    94 => Transaction(inventory::Transaction),
    130 => UseTravelItem(inventory::UseTravelItem),
    286 => SelectTalent(inventory::SelectTalent),
    287 => DeselectTalent(inventory::DeselectTalent),
    313 => UseStickerBookItem(inventory::UseStickerBookItem),
    // social.rs
    159 => GroupList(social::GroupList),
    163 => GroupInvite(social::GroupInvite),
    165 => GroupInvitationResponse(social::GroupInvitationResponse),
    168 => GroupKick(social::GroupKick),
    175 => GuildList(social::GuildList),
    177 => GuildFoundation(social::GuildFoundation),
    185 => BuddyList(social::BuddyList),
    191 => GroupQuery(social::GroupQuery),
    204 => PvpFlag(social::PvpFlag),
    218 => JoinMatchQueue(social::JoinMatchQueue),
}

impl ClientCommand {
    /// The whole 0x8B message and its length in bits.
    pub fn encode(&self) -> (Vec<u8>, usize) {
        let mut w = BitWriter::new();
        w.write_u8(CLIENT);
        w.write_u16(self.id());
        self.encode_body(&mut w);
        w.finish()
    }

    /// Read a 0x8B message back (what the server does with it).
    /// CONTRACT: the body must end within PADDING_BITS of `bits`.
    pub fn decode(payload: &[u8], bits: usize) -> Result<Self, DecodeError> {
        let mut r = BitReader::with_bit_len(payload, bits);
        let envelope = r.read_u8().map_err(|_| DecodeError::Empty)?;
        if envelope != CLIENT {
            return Err(DecodeError::NotACommandMessage(envelope));
        }
        let id = r.read_u16()?;
        let at = r.position();
        let wrap = |error| DecodeError::InCommand { id, index: 0, at, error: Box::new(error) };
        let command = match Self::decode_body(id, &mut r) {
            Some(c) => c.map_err(wrap)?,
            None => {
                let n = r.remaining();
                Self::Unknown { id, body: Raw::read(&mut r, n)? }
            }
        };
        if r.remaining() > PADDING_BITS {
            return Err(wrap(DecodeError::Trailing { bits: r.remaining() }));
        }
        Ok(command)
    }

    /// The class name the 2018 client registers for this command.
    pub fn name(&self) -> &'static str {
        super::command_name(self.id()).unwrap_or("?")
    }
}
