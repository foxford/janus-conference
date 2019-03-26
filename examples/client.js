// JavaScript variables holding stream and connection information
var localStream, remoteStream, peerConnection;

// JavaScript variables associated with HTML5 video elements in the page
var localVideo = document.getElementById("localVideo");
var remoteVideo = document.getElementById("remoteVideo");

// JavaScript variables assciated with call management buttons in the page
var publisherStartButton = document.getElementById("publisherStartButton");
var listenerStartButton = document.getElementById("listenerStartButton");
var hangupButton = document.getElementById("hangupButton");

// Just allow the user to click on the Start or Join button at start-up
publisherStartButton.disabled = false;
listenerStartButton.disabled = false;
hangupButton.disabled = true;

// Associate JavaScript handlers with click events on the buttons
publisherStartButton.onclick = startTranslation;
listenerStartButton.onclick = joinTranslation;
hangupButton.onclick = hangup;

var websocket, sessionId, pluginHandleId, sessionTransaction, handleTransaction;
var janusHost = "ws://192.168.99.100:8188";
var streamId = "demo-conference-stream";

function startTranslation() {
    navigator.mediaDevices.getUserMedia({ audio: true, video: true })
        .then(stream => {
            localStream = stream;
            localVideo.srcObject = stream;

            start(true);
        })
        .catch(error => console.error(error));
}

function joinTranslation() {
    start(false);
}

function start(isPublisher) {
    publisherStartButton.disabled = true;
    listenerStartButton.disabled = true;
    hangupButton.disabled = false;

    var gotLocalDescription = isPublisher ? publisherGotLocalDescription : listenerGotLocalDescription;
    var options = { offerToReceiveVideo: !isPublisher };

    websocket = new WebSocket(janusHost, 'janus-protocol');
    websocket.onopen = function (event) {
        peerConnection = new RTCPeerConnection(null);

        // Triggered whenever a new candidate is made available to the local peer by the ICE protocol machine
        peerConnection.onicecandidate = gotLocalIceCandidate;

        if (isPublisher) {
            peerConnection.addStream(localStream);
        }

        if (!isPublisher) {
            peerConnection.onaddstream = (event) => {
                console.log(event);
                console.log('got remote stream');
                remoteVideo.srcObject = event.stream;
            };
        }

        sessionTransaction = getTransactionId();
        var payload = {
            "janus": "create",
            "transaction": sessionTransaction
        };
        websocket.send(JSON.stringify(payload));
    };

    websocket.onmessage = function (event) {
        var data = JSON.parse(event.data);

        console.info(data);

        switch (data.janus) {
            case 'success':
                if (data.transaction == sessionTransaction) {
                    sessionId = data.data.id;

                    handleTransaction = getTransactionId();
                    var payload = {
                        "janus": "attach",
                        "session_id": sessionId,
                        "plugin": "janus.plugin.conference",
                        "transaction": handleTransaction
                    };
                    websocket.send(JSON.stringify(payload));

                } else if (data.transaction == handleTransaction) {
                    pluginHandleId = data.data.id;

                    console.log('creating offer');
                    peerConnection.createOffer(gotLocalDescription, onSignalingError, options);
                }
                break;

            case 'event':
                var jsep = new RTCSessionDescription(data.jsep);
                console.log(jsep);

                if (jsep.type == 'answer') {
                    console.info("Stream has been started!");
                    peerConnection.setRemoteDescription(jsep);
                }
                break;

            default:
                break;
        }
    }
}

function hangup() {
    peerConnection.close();
    websocket.close();

    localStream = null;
    remoteStream = null;

    publisherStartButton.disabled = false;
    listenerStartButton.disabled = false;
    hangupButton.disabled = true;
}

function gotLocalIceCandidate(event) {
    console.log('gotLocalIceCandidate');
    var candidate = event.candidate;
    console.log(candidate);

    if (candidate) {
        var payload = {
            "janus": "trickle",
            "session_id": sessionId,
            "handle_id": pluginHandleId,
            "transaction": getTransactionId(),
            "candidate": candidate
        };

        console.log('Uploading ICE candidate');
        console.log(payload);
        websocket.send(JSON.stringify(payload));
    }
}

function publisherGotLocalDescription(desc) {
    console.log('got local SDP');

    desc.sdp = desc.sdp.replace("a=rtpmap:96 VP8/90000", "a=rtpmap:96 H264/90000");
    desc.sdp = desc.sdp.replace("a=rtpmap:98 VP9/90000", "a=rtpmap:98 H264/90000");

    console.log(desc);

    peerConnection.setLocalDescription(desc);

    var payload = {
        "janus": "message",
        "session_id": sessionId,
        "handle_id": pluginHandleId,
        "transaction": getTransactionId(),
        "body": {
            "method": "stream.create",
            "id": streamId
        },
        "jsep": {
            "type": "offer",
            "sdp": desc.sdp
        }
    };

    console.log('Uploading stream.create request');
    console.log(payload);
    websocket.send(JSON.stringify(payload));
}

function listenerGotLocalDescription(desc) {
    console.log("got local SDP");
    console.log(desc);

    peerConnection.setLocalDescription(desc);

    var payload = {
        "janus": "message",
        "session_id": sessionId,
        "handle_id": pluginHandleId,
        "transaction": getTransactionId(),
        "body": {
            "method": "stream.read",
            "id": streamId
        },
        "jsep": {
            "type": "offer",
            "sdp": desc.sdp
        }
    };

    console.log('Uploading stream.read request');
    websocket.send(JSON.stringify(payload));
}

function onSignalingError(error) {
    console.log('Failed to create signaling message : ' + error.message);
}

function getTransactionId() {
    return Math.random().toString(36).replace(/[^a-z]+/g, '').substr(0, 5);
}
