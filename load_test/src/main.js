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
const ME_AGENT_ID = `${ME_AGENT_LABEL}.${ME_ACCOUNT_ID}`;;
const PUBLISH_TOPIC = `agents/${JANUS_AGENT_ID}/api/v1/in/${ME_ACCOUNT_ID}`;
const SUBSCRIBE_TOPIC = `agents/${ME_AGENT_ID}/api/v1/in/${JANUS_ACCOUNT_ID}`;
const PLUGIN = 'janus.plugin.conference';
const REQUEST_TIMEOUT = 60000;
const STREAM_ID = '3fdef418-15d3-11ea-9005-60f81db6d53e';
const ICE_SERVERS = [{ urls: ["stun:stun.l.google.com:19302"] }];

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
        username: '',
        clientId: ME_AGENT_ID,
        reconnectPeriod: 0,
        keepalive: 10,
        properties: {
          userProperties: {
            connection_mode: 'service',
            connection_version: 'v2',
          },
        },
      });

      this.client.on('message', this._handleMessage.bind(this))

      this.client.on('connect', async evt => {
        if (this.onConnect) this.onConnect(evt);

        this.client.subscribe(SUBSCRIBE_TOPIC, async err => {
          if (err) return reject(err);
          await this._initSession();
          resolve();
        });
      });
    });
  }

  async _initSession() {
    let sessionResponse = await this._makeRequest({ janus: 'create' });
    if (sessionResponse.status !== 'replied') throw sessionResponse;
    this.sessionId = sessionResponse.payload.data.id;
    console.debug(`Session ID: ${this.sessionId}`);
    if (this.onSessionIdChange) this.onSessionIdChange(this.sessionId);
  }

  async attach(sdpOffer) {
    let handleResponse = await this._makeRequest({ janus: 'attach', plugin: PLUGIN });
    if (handleResponse.status !== 'replied') throw handleResponse;
    let handleId = handleResponse.payload.data.id;

    let payload = { id: STREAM_ID, agent_id: ME_AGENT_ID };
    await this._callMethod('stream.read', handleId, payload, sdpOffer);

    return handleId;
  }

  async detach(handleId) {
    let handleResponse = await this._makeRequest({ janus: 'detach' }, handleId);
    if (handleResponse.status !== 'replied') throw handleResponse;
  }

  async _callMethod(method, handleId, payload, jsep) {
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

  async _makeRequest(payload, handleId) {
    if (this.sessionId) payload.session_id = this.sessionId;
    if (handleId) payload.handle_id = handleId;
    payload.transaction = Math.random().toString(36).substr(2, 10);

    let promise = new Promise((resolve, reject) => {
      let timeoutHandle = setTimeout(() => {
        delete this.pendingTransactions[payload.transaction];
        reject(`Request with transaction ID ${payload.transaction} (${payload.janus}) timed out`);
      }, REQUEST_TIMEOUT);

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
          type: 'request',
          method: `janus.${payload.janus}`,
          local_timestamp: new Date().getTime().toString(),
        },
      }
    });

    return promise;
  }

  _handleMessage(_topic, payloadBytes, _packet) {
    let payload = JSON.parse(payloadBytes);
    console.debug('Incoming message', payload);

    if (payload.transaction && this.pendingTransactions[payload.transaction]) {
      let { resolve, reject, janus } = this.pendingTransactions[payload.transaction];
      if (payload.janus === 'ack' && janus === 'message') return;

      delete this.pendingTransactions[payload.transaction];
      payload.janus === 'error' ? reject(payload) : resolve({ status: 'replied', payload });
    }
  }
}

function percentile(array, q) {
  let data = array.sort((a, b) => a - b);
  let pos = ((data.length) - 1) * q;
  let base = Math.floor(pos);
  let rest = pos - base;

  if ((data[base + 1] !== undefined)) {
    return data[base] + rest * (data[base + 1] - data[base]);
  } else {
    return data[base];
  }
}

document.addEventListener('DOMContentLoaded', async function () {
  document.getElementById('start-btn').addEventListener('click', async function () {
    console.debug('Creating offers');
    let requestsNumber = parseInt(document.getElementById('requests-number').value);
    if (!requestsNumber) throw `Invalid requests number: ${requestsNumber}`;

    let sdpOffers = [];

    for (let i = 0; i < requestsNumber; i++) {
      let pc = new RTCPeerConnection({ bundlePolicy: 'max-bundle', iceServers: ICE_SERVERS });
      let sdpOffer = await pc.createOffer({ offerToReceiveVideo: true, offerToReceiveAudio: true });
      sdpOffer.sdp = transformOfferSDP(sdpOffer.sdp);
      sdpOffers.push(sdpOffer);
      pc.close();
    }

    console.debug('Connecting');
    let janusClient = new JanusClient();
    await janusClient.connect();

    console.debug('Attaching and sending offers');
    let promises = [];
    let promiseProfiles = [];
    let handleIds = [];

    for (let sdpOffer of sdpOffers) {
      let promise = janusClient.attach(sdpOffer).then(handleId => handleIds.push(handleId));
      promises.push({ promise, startedAt: new Date() });
    }

    for (let { promise, startedAt } of promises) {
      try {
        await promise;
        promiseProfiles.push(new Date() - startedAt);
      } catch (e) {
        console.error(e);
      }
    }

    console.log(
      `Got ${handleIds.length} handles`,
      {
        q50: percentile(promiseProfiles, 0.5),
        q90: percentile(promiseProfiles, 0.9),
        q95: percentile(promiseProfiles, 0.95),
        q99: percentile(promiseProfiles, 0.99),
        max: Math.max(...promiseProfiles),
      }
    );

    console.debug('Detaching');
    for (let handleId of handleIds) await janusClient.detach(handleId);
  });
});
