import { parse, write } from 'sdp-transform'

function getPayloadByCodec (codec, rtp) {
  let payload = null
  let i

  for (i = 0; i < rtp.length; i++) {
    if (rtp[i].codec === codec) {
      payload = rtp[i].payload

      break
    }
  }

  return payload
}

function filterByPayload (payload) {
  return function (item) {
    return item.payload === payload
  }
}

function mapByPayload (payload) {
  return function (item) {
    item.payload = payload

    return item
  }
}

export function transformOfferSDP (sdp, opts) {
  const sdpParsed = parse(sdp)
  const config = {
    audio: {
      codecName: 'opus',
      modifiedPayload: 109,
    },
    video: {
      codecName: 'VP8',
      modifiedPayload: 120,
    }
  }

  console.debug('[sdp] opts', opts)
  console.debug('[sdp] original', sdpParsed)

  sdpParsed.media.forEach(m => {
    if (m.type === 'audio' || m.type === 'video') {
      const originalPayload = getPayloadByCodec(config[m.type].codecName, m.rtp)

      console.debug(`[${config[m.type].codecName}] payload: ${originalPayload} --> ${config[m.type].modifiedPayload}`)

      m.rtp = m.rtp.filter(filterByPayload(originalPayload)).map(mapByPayload(config[m.type].modifiedPayload))
      m.fmtp = m.fmtp.filter(filterByPayload(originalPayload)).map(mapByPayload(config[m.type].modifiedPayload))

      if (m.rtcpFb) {
        m.rtcpFb = m.rtcpFb.filter(filterByPayload(originalPayload)).map(mapByPayload(config[m.type].modifiedPayload))
      }

      m.payloads = String(config[m.type].modifiedPayload)
      // m.bandwidth = [{type: 'AS', limit: '20'}]
    }
  })

  if (typeof opts !== 'undefined' && opts.direction) {
    sdpParsed.media = sdpParsed.media.map((it) => {
      it.direction = opts.direction

      return Object.assign({}, it)
    })
  }

  return write(sdpParsed)
}
