#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct Transaction(pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionId(pub u64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandleId(pub u64);

/// Generic janus response.
#[derive(Clone, Debug, Deserialize)]
pub struct GenericResponse {
    pub janus: String,
    pub transaction: Transaction,
}

/// Response for session request.
/// https://docs.netology-group.services/janus-conference/api.intro.html
#[derive(Clone, Debug, Deserialize)]
pub struct SessionResponse {
    pub janus: String,
    pub data: SessionResponseData,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SessionResponseData {
    pub id: SessionId,
}

/// Response for handle request.
/// https://docs.netology-group.services/janus-conference/api.intro.html
#[derive(Clone, Debug, Deserialize)]
pub struct HandleResponse {
    pub janus: String,
    pub data: HandleResponseData,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HandleResponseData {
    pub id: HandleId,
}

// ------------------------------------------------------------------------------------------------

/// `stream.create` response.
/// https://docs.netology-group.services/janus-conference/api.stream.create.html
#[derive(Clone, Debug, Deserialize)]
pub struct CreateResponse {
    pub janus: String,
    pub plugindata: CreateResponsePluginData,
    pub jsep: CreateResponseJsep,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateResponsePluginData {
    pub data: CreateResponsePluginDataData,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateResponsePluginDataData {
    pub status: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateResponseJsep {
    pub r#type: String,
    pub sdp: String,
}

// ------------------------------------------------------------------------------------------------

/// `stream.upload` response
/// https://docs.netology-group.services/janus-conference/api.stream.upload.html
#[derive(Clone, Debug, Deserialize)]
pub struct UploadResponse {
    pub janus: String,
    pub plugindata: UploadResponsePluginData,
}

#[derive(Clone, Debug, Deserialize)]
pub struct UploadResponsePluginData {
    pub data: UploadResponsePluginDataData,
}

#[derive(Clone, Debug, Deserialize)]
pub struct UploadResponsePluginDataData {
    pub status: usize,
    pub time: Vec<(u64, u64)>,
}
