//! REST response DTOs and cached directory records.
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct ChannelInfo {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct TeamRecord {
    pub id: String,
    pub name: String,
    pub channels: Vec<ChannelInfo>,
}

#[derive(Clone, Debug)]
pub struct ChatMemberRec {
    pub mri: String,
    pub object_id: Option<String>,
    pub role: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ChatRecord {
    pub id: String,
    pub title: Option<String>,
    pub is_one_on_one: bool,
    pub members: Vec<ChatMemberRec>,
    pub last_message_time: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DirCache {
    pub teams: Vec<TeamRecord>,
    pub chats: Vec<ChatRecord>,
}

#[derive(Serialize)]
pub struct GroupSummary {
    pub id: String,
    pub kind: String, // "team" | "group_chat"
    pub name: String,
    pub member_count: usize,
    pub channel_count: usize,
}

#[derive(Serialize)]
pub struct GroupDetail {
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<MemberOut>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<Vec<ChannelOut>>,
}

#[derive(Serialize)]
pub struct ChannelOut {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberOut {
    pub mri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactOut {
    pub id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_title: Option<String>,
    pub source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageOut {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_type: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct SendRequest {
    pub text: String,
}
