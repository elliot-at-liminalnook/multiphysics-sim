// Decode a transport envelope once, without changing its request ID or errors.
// The result cache also lets diagnostics observe the same parsed frame.
export function decodeWorkerResult(message) {
  if (Object.hasOwn(message, 'result_json')) {
    if (typeof message.result_json !== 'string' || Object.hasOwn(message, 'result'))
      throw new Error('Invalid JSON worker response envelope');
    const before = message.timing ? performance.now() : 0;
    const result = JSON.parse(message.result_json);
    if (message.timing) message.timing.receive_json_parse_s = (performance.now()-before)/1000;
    message.result = result;
    delete message.result_json;
  } else if (message.timing) message.timing.receive_json_parse_s ??= 0;
  return message.result;
}
