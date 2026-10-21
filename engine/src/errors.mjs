export class EngineError extends Error {
  constructor(code, message, status = 400) {
    super(message);
    this.code = code;
    this.status = status;
  }
}
export function redact(text, secrets = []) {
  let value = String(text);
  for (const secret of secrets.filter(value => typeof value === 'string' && value.length > 0).sort((a, b) => b.length - a.length))
    value = value.split(secret).join('[redacted]');
  return value;
}
