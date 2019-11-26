'use strict';

const MQTT_URL = 'ws://0.0.0.0:8080/mqtt';
const SVC_AUDIENCE = 'dev.svc.example.org';
const JANUS_ACCOUNT_ID = `janus-gateway.${SVC_AUDIENCE}`;
const JANUS_AGENT_ID = `alpha.${JANUS_ACCOUNT_ID}`;
const ME_ACCOUNT_ID = `conference.${SVC_AUDIENCE}`;
const ME_AGENT_LABEL = Math.random().toString(36).substr(2, 10);
const ME_CLIENT_ID = `v1/service-agents/${ME_AGENT_LABEL}.${ME_ACCOUNT_ID}`;
const PUBLISH_TOPIC = `agents/${JANUS_AGENT_ID}/api/v1/in/${ME_ACCOUNT_ID}`;
const SUBSCRIBE_TOPIC = `apps/${JANUS_ACCOUNT_ID}/api/v1/responses`;
const PLUGIN = 'janus.plugin.conference';
const STREAM_ID = 'demo-conference-stream';
const CONSTRAINTS = { audio: true, video: { width: 1280, height: 720 } };

///////////////////////////////////////////////////////////////////////////////

class JanusClient {
  constructor() {
    this.client = null;
    this.sessionId = null;
    this.handleId = null;
    this.pendingTransactions = {};
  }

  async connect() {
    return new Promise((resolve, reject) => {
      this.client = new mqtt.connect(MQTT_URL, {
        protocolVersion: 5,
        clientId: ME_CLIENT_ID,
        reconnectPeriod: 0
      });

      this.client.on('message', this._handleMessage.bind(this))

      this.client.on('connect', async () => {
        this.client.subscribe(SUBSCRIBE_TOPIC, async err => {
          if (err) return reject(err);

          let sessionResponse = await this._makeRequest({ janus: 'create' });
          this.sessionId = sessionResponse.data.id;
          console.debug(`Session ID: ${this.sessionId}`);

          let handleResponse = await this._makeRequest({ janus: 'attach', plugin: PLUGIN });
          this.handleId = handleResponse.data.id;
          console.debug(`Handle ID: ${this.handleId}`);

          resolve();
        });
      });
    });
  }

  async makeRequest(payload) {
    if (this.handleId) {
      return this._makeRequest(payload);
    } else {
      throw 'Expected to await on `JanusClient#connect` before making any requests';
    }
  }

  async _makeRequest(payload) {
    if (this.sessionId) payload.session_id = this.sessionId;
    if (this.handleId) payload.handle_id = this.handleId;
    payload.transaction = Math.random().toString(36).substr(2, 10);

    let promise = new Promise((resolve, reject) => {
      this.pendingTransactions[payload.transaction] = { resolve, reject, janus: payload.janus };

      setTimeout(() => {
        delete this.pendingTransactions[payload.transaction];
        reject(`Request with transaction ID ${payload.transaction} timed out`);
      }, 60000);
    });

    console.debug('Outgoing message', payload);

    this.client.publish(PUBLISH_TOPIC, JSON.stringify(payload), {
      properties: {
        responseTopic: SUBSCRIBE_TOPIC,
        correlationData: payload.transaction,
      }
    });

    return promise;
  }

  async callMethod(method, payload, jsep) {
    let response = await this.makeRequest({
      janus: 'message',
      body: { ...payload, method },
      jsep
    });

    let data = response.plugindata.data;

    if (data.status === '200') {
      return response;
    } else {
      throw `${data.status} ${data.title}: ${data.detail} (${response.transaction})`;
    }
  }

  disconnect() {
    this.client.end();
  }

  _handleMessage(_topic, payloadBytes, _packet) {
    let payload = JSON.parse(payloadBytes);
    console.debug('Incoming message', payload);

    if (payload.transaction && this.pendingTransactions[payload.transaction]) {
      let { resolve, reject, janus } = this.pendingTransactions[payload.transaction];
      if (payload.janus === 'ack' && janus === 'message') return;

      delete this.pendingTransactions[payload.transaction];
      payload.janus === 'error' ? reject(payload) : resolve(payload);
    }
  }
}

///////////////////////////////////////////////////////////////////////////////

class Peer {
  constructor() {
    this.janusClient = null;
    this._resetPeerConnection();
  }

  _resetPeerConnection() {
    this.peerConnection = new RTCPeerConnection({ bundlePolicy: 'max-bundle' });

    this.peerConnection.onicecandidate = evt => {
      if (!evt.candidate) return;
      this.janusClient.makeRequest({ janus: 'trickle', candidate: evt.candidate });
    }

    this.peerConnection.onaddstream = evt => {
      this.onStreamAdded && this.onStreamAdded(evt.stream);
    }

    this.peerConnection.onconnectionstatechange = evt => {
      this.onConnectionStateChange && this.onConnectionStateChange(evt.target.connectionState);
    };

    this.peerConnection.onsignalingstatechange = evt => {
      this.onSignalingStateChange && this.onSignalingStateChange(evt.target.signalingState);
    };

    this.peerConnection.onicegatheringstatechange = evt => {
      this.onIceGatheringStateChange && this.onIceGatheringStateChange(evt.target.iceGatheringState);
    };
  }

  addStream(stream) {
    this.peerConnection.addStream(stream);
  }

  async attach(isPublisher) {
    this.janusClient = new JanusClient();
    await this.janusClient.connect();

    let sdpOffer = await this._createSdpOffer(isPublisher);
    this.peerConnection.setLocalDescription(sdpOffer);

    let method = isPublisher ? 'stream.create' : 'stream.read';
    let response = await this.janusClient.callMethod(method, { id: STREAM_ID }, sdpOffer);

    let sdpAnswer = new RTCSessionDescription(response.jsep);
    this.peerConnection.setRemoteDescription(sdpAnswer);
  }

  hangUp() {
    this.janusClient.disconnect();
    this.peerConnection.close();
    this._resetPeerConnection();
  }

  async _createSdpOffer(isPublisher) {
    return new Promise((resolve, reject) => {
      this.peerConnection.createOffer(
        sdpOffer => {
          // Replace VP8/VP9 codecs with H264.
          sdpOffer.sdp = sdpOffer.sdp.replace("a=rtpmap:96 VP8/90000", "a=rtpmap:96 H264/90000");
          sdpOffer.sdp = sdpOffer.sdp.replace("a=rtpmap:98 VP9/90000", "a=rtpmap:98 H264/90000");
          resolve(sdpOffer);
        },
        err => reject(err),
        { offerToReceiveVideo: !isPublisher }
      );
    });
  }
}

///////////////////////////////////////////////////////////////////////////////

document.addEventListener('DOMContentLoaded', function () {
  let videoEl = document.getElementById('video');
  let startBtn = document.getElementById('startBtn');
  let joinBtn = document.getElementById('joinBtn');
  let hangUpBtn = document.getElementById('hangUpBtn');
  let connectionStateIndicator = document.getElementById('connectionStateIndicator');
  let signalingStateIndicator = document.getElementById('signalingStateIndicator');
  let iceGatheringStateIndicator = document.getElementById('iceGatheringStateIndicator');

  let peer = new Peer();
  peer.onConnectionStateChange = state => connectionStateIndicator.innerHTML = state;
  peer.onSignalingStateChange = state => signalingStateIndicator.innerHTML = state;
  peer.onIceGatheringStateChange = state => iceGatheringStateIndicator.innerHTML = state;

  startBtn.addEventListener('click', async function () {
    const stream = await navigator.mediaDevices.getUserMedia(CONSTRAINTS);
    videoEl.srcObject = stream;

    peer.addStream(stream);
    peer.attach(true);

    startBtn.disabled = true;
    joinBtn.disabled = true;
    hangUpBtn.disabled = false;
  });

  joinBtn.addEventListener('click', async function () {
    peer.onStreamAdded = stream => videoEl.srcObject = stream;
    peer.attach(false);

    startBtn.disabled = true;
    joinBtn.disabled = true;
    hangUpBtn.disabled = false;
  });

  hangUpBtn.addEventListener('click', async function () {
    videoEl.srcObject = null;
    peer.hangUp();

    startBtn.disabled = false;
    joinBtn.disabled = false;
    hangUpBtn.disabled = true;

    connectionStateIndicator.innerHTML = 'null';
    signalingStateIndicator.innerHTML = 'null';
    iceGatheringStateIndicator.innerHTML = 'null';
  });
});
