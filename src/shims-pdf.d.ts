/**
 * 类型声明：让 vue-tsc 接受这些无类型的特殊 import
 */
declare module "pdfmake/build/pdfmake.js" {
  interface PdfFonts {
    [family: string]: {
      normal: string;
      bold: string;
      italics: string;
      bolditalics: string;
    };
  }
  interface PdfDoc {
    getBlob: (cb: (b: Blob) => void) => void;
    download: (name?: string) => void;
  }
  interface PdfMakeApi {
    virtualfs: Record<string, string>;
    fonts: PdfFonts;
    addVirtualFileSystem: (files: Record<string, string>) => void;
    setFonts: (fonts: PdfFonts) => void;
    createPdf: (def: unknown) => PdfDoc;
  }
  // Vite/webpack 把 module 包成 { p: <webpack_module> }，
  // <webpack_module>.default 才是真正的 pdfmake 单例
  interface PdfMakeWebpackModule {
    default: PdfMakeApi;
  }
  interface PdfMakeRootModule {
    p?: PdfMakeWebpackModule;
    default?: PdfMakeApi;
  }
  const pdfMakeModule: PdfMakeRootModule;
  export = pdfMakeModule;
}

// Vite 资源 import '*.ttf?url' → 字符串 URL
declare module "*.ttf?url" {
  const url: string;
  export default url;
}
declare module "*.woff?url" {
  const url: string;
  export default url;
}
declare module "*.woff2?url" {
  const url: string;
  export default url;
}