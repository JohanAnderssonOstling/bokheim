const unsupported = () => {
  throw new Error("This mobile reader currently supports EPUB files only");
};

export const makePDF = unsupported;
export const makeComicBook = unsupported;
export const makeFB2 = unsupported;
export const isMOBI = async () => false;
export class MOBI {
  constructor() {
    unsupported();
  }
}
