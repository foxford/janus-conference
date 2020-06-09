'use strict';

import mqtt from 'mqtt';
import { transformOfferSDP } from './sdp';

///////////////////////////////////////////////////////////////////////////////

const MQTT_URL = 'ws://0.0.0.0:8080/mqtt';
const SVC_AUDIENCE = 'dev.svc.example.org';
const JANUS_ACCOUNT_ID = `janus-gateway.${SVC_AUDIENCE}`;
const JANUS_AGENT_ID = `alpha.${JANUS_ACCOUNT_ID}`;
const ME_ACCOUNT_ID = `conference.${SVC_AUDIENCE}`;
const ME_AGENT_LABEL = Math.random().toString(36).substr(2, 10);
const ME_AGENT_ID = `${ME_AGENT_LABEL}.${ME_ACCOUNT_ID}`;
const ME_CLIENT_ID = `v1/service-agents/${ME_AGENT_ID}`;
const PUBLISH_TOPIC = `agents/${JANUS_AGENT_ID}/api/v1/in/${ME_ACCOUNT_ID}`;
const SUBSCRIBE_TOPIC = `apps/${JANUS_ACCOUNT_ID}/api/v1/responses`;
const PLUGIN = 'janus.plugin.conference';
const STREAM_ID = '3fdef418-15d3-11ea-9005-60f81db6d53e';
const CONSTRAINTS = { audio: true, video: { width: 1280, height: 720 } };
const BUCKET = 'origin.webinar.beta.example.org';

///////////////////////////////////////////////////////////////////////////////

class JanusClient {
  constructor() {
    this.client = null;
    this.clientHandleId = null;
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

      this.client.on('connect', async evt => {
        if (this.onConnect) this.onConnect(evt);

        this.client.subscribe(SUBSCRIBE_TOPIC, async err => {
          if (err) return reject(err);
          await this._initSession();
          await this._initclientHandle();
          resolve();
        });
      });

      this.client.on('offline', evt => this.onDisconnect && this.onDisconnect(evt));
    });
  }

  async _initSession() {
    let sessionResponse = await this._makeRequest({ janus: 'create' });
    if (sessionResponse.status !== 'replied') throw sessionResponse;
    this.sessionId = sessionResponse.payload.data.id;
    console.debug(`Session ID: ${this.sessionId}`);
    if (this.onSessionIdChange) this.onSessionIdChange(this.sessionId);
  }

  async _initclientHandle() {
    this.clientHandleId = await this.createHandle();;
    console.debug(`client handle ID: ${this.clientHandleId}`);
    if (this.onclientHandleIdChange) this.onclientHandleIdChange(this.clientHandleId);
  }

  async createHandle() {
    let handleResponse = await this._makeRequest({ janus: 'attach', plugin: PLUGIN });
    if (handleResponse.status !== 'replied') throw handleResponse;
    return handleResponse.payload.data.id
  }

  async _makeRequest(payload, handleId) {
    if (this.sessionId) payload.session_id = this.sessionId;

    if (handleId) {
      payload.handle_id = handleId;
    } else if (this.clientHandleId) {
      payload.handle_id = this.handleclientId;
    }

    payload.transaction = Math.random().toString(36).substr(2, 10);

    let promise = new Promise((resolve, reject) => {
      let timeoutHandle = setTimeout(() => {
        delete this.pendingTransactions[payload.transaction];
        reject(`Request with transaction ID ${payload.transaction} timed out`);
      }, 60000);

      this.pendingTransactions[payload.transaction] = {
        resolve,
        reject,
        janus: payload.janus,
        timeoutHandle
      };
    });

    console.debug('Outgoing message', payload);

    this.client.publish(PUBLISH_TOPIC, JSON.stringify(payload), {
      properties: {
        responseTopic: SUBSCRIBE_TOPIC,
        correlationData: payload.transaction,
        userProperties: {
          local_timestamp: new Date().getTime().toString(),
        },
      }
    });

    return promise;
  }

  async callMethod(method, handleId, payload, jsep) {
    let requestPayload = { janus: 'message', body: { ...payload, method } };
    if (jsep) requestPayload.jsep = jsep;

    let result = await this._makeRequest(requestPayload, handleId);
    if (result.status !== 'replied') return;
    
    let data = result.payload.plugindata.data;

    if (data.status === '200') {
      return result.payload;
    } else {
      throw `${data.status} ${data.title}: ${data.detail} (${result.payload.transaction})`;
    }
  }

  async trickle(candidate, handleId) {
    return await this._makeRequest({ janus: 'trickle', candidate }, handleId);
  }

  _handleMessage(_topic, payloadBytes, _packet) {
    let payload = JSON.parse(payloadBytes);
    console.debug('Incoming message', payload);

    if (payload.transaction && this.pendingTransactions[payload.transaction]) {
      let { resolve, reject, janus } = this.pendingTransactions[payload.transaction];
      if (payload.janus === 'ack' && janus === 'message') return;

      delete this.pendingTransactions[payload.transaction];
      payload.janus === 'error' ? reject(payload) : resolve({ status: 'replied', payload });
    } else if (payload.janus === 'hangup') {
      this._resetPendingTransactions();
    }
  }

  _resetPendingTransactions() {
    for (let { resolve, timeoutHandle } of Object.values(this.pendingTransactions)) {
      clearInterval(timeoutHandle);
      resolve({ status: 'detached' });
    }

    this.pendingTransactions = {};
  }
}

///////////////////////////////////////////////////////////////////////////////

class Peer {
  constructor(janusClient) {
    this.janusClient = janusClient;
    this.handleId = null;
    this._resetPeerConnection();
  }

  _resetPeerConnection() {
    this.peerConnection = new RTCPeerConnection({ bundlePolicy: 'max-bundle' });

    this.peerConnection.onicecandidate = evt => {
      if (!evt.candidate) return;
      this.janusClient.trickle(evt.candidate, this.handleId);
    }

    this.peerConnection.ontrack = evt => {
      this.onStreamAdded && this.onStreamAdded(evt.streams[0]);
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
    this._setHandleId(await this.janusClient.createHandle());

    let sdpOffer = await this._createSdpOffer(isPublisher);
    this.peerConnection.setLocalDescription(sdpOffer);

    let method = isPublisher ? 'stream.create' : 'stream.read';
    let payload = { id: STREAM_ID, agent_id: ME_AGENT_ID };
    let response = await this.janusClient.callMethod(method, this.handleId, payload, sdpOffer);

    let sdpAnswer = new RTCSessionDescription(response.jsep);
    this.peerConnection.setRemoteDescription(sdpAnswer);
  }

  async hangUp() {
    this.peerConnection.close();
    this._setHandleId(null);
    this._resetPeerConnection();
  }

  async upload() {
    let response = await this.janusClient.callMethod('stream.upload', this.handleId, {
      id: STREAM_ID,
      bucket: BUCKET,
      object: `${STREAM_ID}.source.mp4`,
    });

    console.debug('Upload response', response.plugindata.data);
  }

  _setHandleId(handleId) {
    this.handleId = handleId;
    if (this.onHandleIdChange) this.onHandleIdChange(handleId);
  }

  async _createSdpOffer(isPublisher) {
    return this.peerConnection
      .createOffer({
        offerToReceiveVideo: !isPublisher,
        offerToReceiveAudio: !isPublisher
      })
      .then(sdpOffer => {
        sdpOffer.sdp = transformOfferSDP(sdpOffer.sdp);
        return sdpOffer;
      });
  }
}

///////////////////////////////////////////////////////////////////////////////

document.addEventListener('DOMContentLoaded', async function () {
  // Getting DOM elements
  let videoEl = document.getElementById('video');
  let connectBtn = document.getElementById('connectBtn');
  let startBtn = document.getElementById('startBtn');
  let joinBtn = document.getElementById('joinBtn');
  let hangUpBtn = document.getElementById('hangUpBtn');
  let uploadBtn = document.getElementById('uploadBtn');
  let mqttStateIndicator = document.getElementById('mqttStateIndicator');
  let sessionIdIndicator = document.getElementById('sessionIdIndicator');
  let clientHandleIdIndicator = document.getElementById('clientHandleIdIndicator');
  let peerHandleIdIndicator = document.getElementById('peerHandleIdIndicator');
  let connectionStateIndicator = document.getElementById('connectionStateIndicator');
  let signalingStateIndicator = document.getElementById('signalingStateIndicator');
  let iceGatheringStateIndicator = document.getElementById('iceGatheringStateIndicator');

  let peer;

  // Connect button click
  connectBtn.addEventListener('click', async function () {
    connectBtn.disabled = true;

    let janusClient = new JanusClient();
    janusClient.onConnect = () => mqttStateIndicator.innerHTML = 'connected';
    janusClient.onDisconnect = () => mqttStateIndicator.innerHTML = 'not connected';
    janusClient.onSessionIdChange = id => sessionIdIndicator.innerHTML = id || 'null';
    janusClient.onclientHandleIdChange = id => clientHandleIdIndicator.innerHTML = id || 'null';
    await janusClient.connect();

    peer = new Peer(janusClient);
    peer.onHandleIdChange = id => peerHandleIdIndicator.innerHTML = id || 'null';
    peer.onConnectionStateChange = state => connectionStateIndicator.innerHTML = state;
    peer.onSignalingStateChange = state => signalingStateIndicator.innerHTML = state;
    peer.onIceGatheringStateChange = state => iceGatheringStateIndicator.innerHTML = state;

    startBtn.disabled = false;
    joinBtn.disabled = false;
    uploadBtn.disabled = false;
  });

  // Start button click
  startBtn.addEventListener('click', async function () {
    startBtn.disabled = true;
    joinBtn.disabled = true;

    const stream = await navigator.mediaDevices.getUserMedia(CONSTRAINTS);
    videoEl.srcObject = stream;

    peer.addStream(stream);
    await peer.attach(true);

    hangUpBtn.disabled = false;
  });

  // Join button click
  joinBtn.addEventListener('click', async function () {
    startBtn.disabled = true;
    joinBtn.disabled = true;

    peer.onStreamAdded = stream => videoEl.srcObject = stream;
    await peer.attach(false);

    hangUpBtn.disabled = false;
  });

  // Hang up button click
  hangUpBtn.addEventListener('click', async function () {
    hangUpBtn.disabled = true;

    videoEl.srcObject = null;
    await peer.hangUp();

    startBtn.disabled = false;
    joinBtn.disabled = false;    

    connectionStateIndicator.innerHTML = 'null';
    signalingStateIndicator.innerHTML = 'null';
    iceGatheringStateIndicator.innerHTML = 'null';
  });

  uploadBtn.addEventListener('click', async function () {
    await peer.upload();
  });
});
