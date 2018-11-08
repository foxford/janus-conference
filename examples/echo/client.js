// JavaScript variables holding stream and connection information
var localStream, remoteStream, peerConnection;

// JavaScript variables associated with HTML5 video elements in the page
var localVideo = document.getElementById("localVideo");
var remoteVideo = document.getElementById("remoteVideo");

// JavaScript variables assciated with call management buttons in the page
var startButton = document.getElementById("startButton");
var callButton = document.getElementById("callButton");
var hangupButton = document.getElementById("hangupButton");

// Just allow the user to click on the Call button at start-up
startButton.disabled = false;
callButton.disabled = true;
hangupButton.disabled = true;

// Associate JavaScript handlers with click events on the buttons
startButton.onclick = start;
callButton.onclick = call;
hangupButton.onclick = hangup;

var websocket, sessionId, pluginHandleId, sessionTransaction, handleTransaction;
var janusHost = "ws://localhost:8188";

function start() {
    navigator.mediaDevices.getUserMedia({ video: true })
        .then(stream => {
            localStream = stream;
            localVideo.srcObject = stream;
        })
        .catch(error => console.error(error));

    startButton.disabled = true;
    callButton.disabled = false;
}

function call() {
    callButton.disabled = true;
    hangupButton.disabled = false;

    websocket = new WebSocket(janusHost, 'janus-protocol');
    websocket.onopen = function (event) {
        peerConnection = new RTCPeerConnection(null);

        // Triggered whenever a new candidate is made available to the local peer by the ICE protocol machine
        peerConnection.onicecandidate = gotLocalIceCandidate;

        // Triggered on setRemoteDescription() call
        peerConnection.onaddstream = (event) => {
            console.log('got remote stream');
            remoteVideo.srcObject = event.stream;
        };

        peerConnection.addStream(localStream);

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
                    peerConnection.createOffer(gotLocalDescription, onSignalingError);
                }
                break;

            case 'event':
                handleEvent(data);
                break;

            default:
                break;
        }
    }
}

function handleEvent(data) {
    var jsep = new RTCSessionDescription(data.jsep);
    console.log(jsep);

    if (jsep.type == 'answer') {
        peerConnection.setRemoteDescription(jsep);
    }
}

function hangup() {
    peerConnection.close();
    websocket.close();

    localStream = null;
    remoteStream = null;

    startButton.disabled = false;
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

function gotLocalDescription(desc) {
    console.log('got local SDP');
    console.log(desc);

    peerConnection.setLocalDescription(desc);

    var payload = {
        "janus": "message",
        "session_id": sessionId,
        "handle_id": pluginHandleId,
        "transaction": getTransactionId(),
        "body": {
            "video": true
        },
        "jsep": {
            "type": "offer",
            "sdp": desc.sdp
        }
    };

    console.log('Uploading offer');
    console.log(payload);
    websocket.send(JSON.stringify(payload));
}

function onSignalingError(error) {
    console.log('Failed to create signaling message : ' + error.message);
}

function getTransactionId() {
    return Math.random().toString(36).replace(/[^a-z]+/g, '').substr(0, 5);
}