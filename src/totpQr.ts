const MAX_IMAGE_BYTES = 8 * 1024 * 1024
const MAX_IMAGE_PIXELS = 12_000_000
const BASE32 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'
const RASTER_IMAGE_TYPES = new Set(['image/png', 'image/jpeg', 'image/webp', 'image/gif', 'image/bmp'])

function normalizeSecret(value: string): string {
  const secret = value.replace(/\s/g, '').replace(/=+$/, '').toUpperCase()
  if (!/^[A-Z2-7]{16,128}$/.test(secret) || ![0, 2, 4, 5, 7].includes(secret.length % 8)) {
    throw new Error('二维码中没有有效的动态密钥')
  }
  const remainder = secret.length % 8 * 5 % 8
  const finalValue = BASE32.indexOf(secret.at(-1)!)
  if (remainder && finalValue % 2 ** remainder !== 0) throw new Error('二维码中没有有效的动态密钥')
  return secret
}

export function extractTotpSecret(payload: string): string {
  if (typeof payload !== 'string' || !payload || payload.length > 8192) {
    throw new Error('二维码中没有有效的动态密钥')
  }

  if (/^otpauth:/i.test(payload)) {
    let uri: URL
    try { uri = new URL(payload) }
    catch { throw new Error('不支持的二维码') }
    if (uri.protocol.toLowerCase() !== 'otpauth:' || uri.hostname.toLowerCase() !== 'totp') {
      throw new Error('不支持的二维码')
    }
    if ((uri.searchParams.get('algorithm') ?? 'SHA1').toUpperCase() !== 'SHA1'
      || (uri.searchParams.get('digits') ?? '6') !== '6'
      || (uri.searchParams.get('period') ?? '30') !== '30') {
      throw new Error('不支持的二维码')
    }
    if (uri.searchParams.getAll('secret').length !== 1) throw new Error('二维码中没有有效的动态密钥')
    return normalizeSecret(uri.searchParams.get('secret') ?? '')
  }

  if (/^[a-z][a-z\d+.-]*:/i.test(payload)) throw new Error('不支持的二维码')
  return normalizeSecret(payload)
}

function readDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onerror = () => reject(new Error('无法读取二维码图片'))
    reader.onload = () => typeof reader.result === 'string' ? resolve(reader.result) : reject(new Error('无法读取二维码图片'))
    reader.readAsDataURL(file)
  })
}

export async function readTotpSecretFromImage(file: File): Promise<string> {
  if ((file.type && !RASTER_IMAGE_TYPES.has(file.type.toLowerCase())) || !file.size || file.size > MAX_IMAGE_BYTES) {
    throw new Error('请选择 8 MB 以内的二维码图片')
  }

  try {
    const image = new Image()
    image.src = await readDataUrl(file)
    await image.decode()
    if (!image.naturalWidth || !image.naturalHeight || image.naturalWidth * image.naturalHeight > MAX_IMAGE_PIXELS) {
      throw new Error('二维码图片尺寸过大')
    }
    const { BrowserQRCodeReader } = await import('@zxing/browser')
    const result = await new BrowserQRCodeReader().decodeFromImageElement(image)
    return extractTotpSecret(result.getText())
  } catch (error) {
    if (error instanceof Error && /^(不支持的二维码|二维码中没有有效的动态密钥|请选择 8 MB 以内的二维码图片|二维码图片尺寸过大)$/.test(error.message)) {
      throw error
    }
    throw new Error('无法识别二维码，或二维码中没有有效的 TOTP 密钥')
  }
}
