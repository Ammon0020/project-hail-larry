declare module 'utif' {
  interface IFD {
    width: number
    height: number
    [key: string]: unknown
  }
  export function decode(buf: ArrayBuffer): IFD[]
  export function decodeImage(buf: ArrayBuffer, ifd: IFD): void
  export function toRGBA8(ifd: IFD): Uint8Array
}
