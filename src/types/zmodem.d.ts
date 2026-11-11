// zmodem.js ships without types; termTransfers.ts declares the parts it uses.
declare module "zmodem.js" {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const Zmodem: any;
  export default Zmodem;
}
