use crate::function::ComputeFunction;
use std::sync::Arc;

pub fn register_format_functions() -> Vec<Arc<dyn ComputeFunction>> {
    let mut fns: Vec<Arc<dyn ComputeFunction>> = Vec::new();

    #[cfg(feature = "format-detect")]
    {
        use crate::builtins_format_detect::*;
        fns.push(Arc::new(FormatDetectFn));
        fns.push(Arc::new(FormatValidateFn));
        fns.push(Arc::new(UniversalMetadataFn));
        fns.push(Arc::new(UniversalToJsonFn));
        fns.push(Arc::new(UniversalToTextFn));
        fns.push(Arc::new(FormatConvertFn));
        fns.push(Arc::new(SchemaInferFn));
        fns.push(Arc::new(SchemaCompareFn));
        fns.push(Arc::new(MimeTypeMapFn));
        fns.push(Arc::new(EncodingDetectFn));
        fns.push(Arc::new(IsTextFn));
        fns.push(Arc::new(IsBinaryFn));
        fns.push(Arc::new(IsCompressedFn));
        fns.push(Arc::new(IsEncryptedFn));
        fns.push(Arc::new(ByteHistogramFn));
        fns.push(Arc::new(EntropyScoreFn));
        fns.push(Arc::new(StructureHeuristicFn));
    }

    #[cfg(feature = "format-csv")]
    {
        use crate::builtins_format_csv::*;
        fns.push(Arc::new(CsvParseFn));
        fns.push(Arc::new(CsvWriteFn));
        fns.push(Arc::new(CsvSchemaInferFn));
        fns.push(Arc::new(CsvColumnSelectFn));
        fns.push(Arc::new(CsvColumnRenameFn));
        fns.push(Arc::new(CsvFilterFn));
        fns.push(Arc::new(CsvSortFn));
        fns.push(Arc::new(CsvAggregateFn));
        fns.push(Arc::new(CsvJoinFn));
        fns.push(Arc::new(CsvDeduplicateFn));
        fns.push(Arc::new(TsvParseFn));
        fns.push(Arc::new(NdjsonParseFn));
        fns.push(Arc::new(NdjsonWriteFn));
        fns.push(Arc::new(NdjsonFilterFn));
        fns.push(Arc::new(NdjsonProjectFn));
        fns.push(Arc::new(JsonPathExtractFn));
        fns.push(Arc::new(JsonMergeFn));
        fns.push(Arc::new(JsonFlattenFn));
        fns.push(Arc::new(JsonUnflattenFn));
        fns.push(Arc::new(XmlParseFn));
        fns.push(Arc::new(XmlWriteFn));
        fns.push(Arc::new(XmlXPathExtractFn));
        fns.push(Arc::new(XmlXsltTransformFn));
        fns.push(Arc::new(XmlValidateDtdFn));
        fns.push(Arc::new(XmlValidateXsdFn));
    }

    #[cfg(feature = "format-config")]
    {
        use crate::builtins_format_config::*;
        fns.push(Arc::new(YamlParseFn));
        fns.push(Arc::new(YamlWriteFn));
        fns.push(Arc::new(YamlValidateFn));
        fns.push(Arc::new(YamlMergeFn));
        fns.push(Arc::new(TomlParseFn));
        fns.push(Arc::new(TomlWriteFn));
        fns.push(Arc::new(IniParseFn));
        fns.push(Arc::new(IniWriteFn));
        fns.push(Arc::new(EnvParseFn));
        fns.push(Arc::new(EnvWriteFn));
        fns.push(Arc::new(HclParseFn));
        fns.push(Arc::new(PropertiesParseFn));
        fns.push(Arc::new(PropertiesWriteFn));
        fns.push(Arc::new(PlistParseFn));
        fns.push(Arc::new(PlistWriteFn));
        fns.push(Arc::new(ConfigFormatConvertFn));
    }

    #[cfg(feature = "format-archive")]
    {
        use crate::builtins_format_archive::*;
        fns.push(Arc::new(TarCreateFn));
        fns.push(Arc::new(TarExtractFn));
        fns.push(Arc::new(TarListFn));
        fns.push(Arc::new(TarAppendFn));
        fns.push(Arc::new(GzipCompressFn));
        fns.push(Arc::new(GzipDecompressFn));
        fns.push(Arc::new(Bzip2CompressFn));
        fns.push(Arc::new(Bzip2DecompressFn));
        fns.push(Arc::new(XzCompressFn));
        fns.push(Arc::new(XzDecompressFn));
        fns.push(Arc::new(ZipCreateFn));
        fns.push(Arc::new(ZipExtractFn));
        fns.push(Arc::new(ZipListFn));
        fns.push(Arc::new(ZipExtractSingleFn));
        fns.push(Arc::new(TarGzCreateFn));
        fns.push(Arc::new(TarGzExtractFn));
        fns.push(Arc::new(ZstdFrameCompressFn));
        fns.push(Arc::new(ZstdFrameDecompressFn));
        fns.push(Arc::new(SevenZExtractFn));
        fns.push(Arc::new(RarExtractFn));
    }

    #[cfg(feature = "format-log")]
    {
        use crate::builtins_format_log::*;
        fns.push(Arc::new(SyslogParseFn));
        fns.push(Arc::new(SyslogWriteFn));
        fns.push(Arc::new(CefParseFn));
        fns.push(Arc::new(ElfParseFn));
        fns.push(Arc::new(ApacheLogParseFn));
        fns.push(Arc::new(NginxLogParseFn));
        fns.push(Arc::new(CloudTrailParseFn));
        fns.push(Arc::new(VpcFlowLogParseFn));
        fns.push(Arc::new(PrometheusExpositionParseFn));
        fns.push(Arc::new(OtlpDecodeFn));
        fns.push(Arc::new(LogTimestampNormalizeFn));
        fns.push(Arc::new(LogLevelFilterFn));
        fns.push(Arc::new(LogAnonymizeFn));
    }

    #[cfg(feature = "format-cas")]
    {
        use crate::builtins_format_cas::*;
        fns.push(Arc::new(CidComputeFn));
        fns.push(Arc::new(CidVerifyFn));
        fns.push(Arc::new(CidParseFn));
        fns.push(Arc::new(DagPbEncodeFn));
        fns.push(Arc::new(DagPbDecodeFn));
        fns.push(Arc::new(DagCborEncodeFn));
        fns.push(Arc::new(DagCborDecodeFn));
        fns.push(Arc::new(CarCreateFn));
        fns.push(Arc::new(CarExtractFn));
        fns.push(Arc::new(CarListFn));
        fns.push(Arc::new(CarVerifyFn));
        fns.push(Arc::new(UnixFsChunkFn));
        fns.push(Arc::new(UnixFsAssembleFn));
        fns.push(Arc::new(MerkleProofGenerateFn));
        fns.push(Arc::new(MerkleProofVerifyFn));
    }

    #[cfg(feature = "format-erasure")]
    {
        use crate::builtins_format_erasure::*;
        fns.push(Arc::new(ReedSolomonEncodeFn));
        fns.push(Arc::new(ReedSolomonDecodeFn));
        fns.push(Arc::new(ReedSolomonVerifyFn));
        fns.push(Arc::new(XorParityFn));
        fns.push(Arc::new(XorReconstructFn));
        fns.push(Arc::new(ReplicationSplitFn));
        fns.push(Arc::new(ReplicationVerifyFn));
        fns.push(Arc::new(StripeSplitFn));
        fns.push(Arc::new(StripeAssembleFn));
    }

    #[cfg(feature = "format-serialization")]
    {
        use crate::builtins_format_serialization::*;
        fns.push(Arc::new(AvroReadFn));
        fns.push(Arc::new(AvroWriteFn));
        fns.push(Arc::new(AvroSchemaExtractFn));
        fns.push(Arc::new(AvroSchemaEvolveFn));
        fns.push(Arc::new(AvroToJsonFn));
        fns.push(Arc::new(JsonToAvroFn));
        fns.push(Arc::new(ProtobufDecodeFn));
        fns.push(Arc::new(ProtobufEncodeFn));
        fns.push(Arc::new(ProtobufSchemaExtractFn));
        fns.push(Arc::new(ThriftDecodeFn));
        fns.push(Arc::new(ThriftEncodeFn));
        fns.push(Arc::new(MsgpackDecodeFn));
        fns.push(Arc::new(MsgpackEncodeFn));
        fns.push(Arc::new(CborDecodeFn));
        fns.push(Arc::new(CborEncodeFn));
        fns.push(Arc::new(BsonDecodeFn));
        fns.push(Arc::new(BsonEncodeFn));
        fns.push(Arc::new(AvroToParquetFn));
        fns.push(Arc::new(ParquetToAvroFn));
    }

    #[cfg(feature = "format-columnar")]
    {
        use crate::builtins_format_columnar::*;
        fns.push(Arc::new(ParquetReadFn));
        fns.push(Arc::new(ParquetWriteFn));
        fns.push(Arc::new(ParquetMetadataFn));
        fns.push(Arc::new(ParquetProjectionFn));
        fns.push(Arc::new(ParquetFilterFn));
        fns.push(Arc::new(ParquetMergeFn));
        fns.push(Arc::new(ParquetToArrowFn));
        fns.push(Arc::new(ArrowToParquetFn));
        fns.push(Arc::new(OrcReadFn));
        fns.push(Arc::new(OrcWriteFn));
        fns.push(Arc::new(OrcMetadataFn));
        fns.push(Arc::new(OrcFilterFn));
        fns.push(Arc::new(ArrowIpcReadFn));
        fns.push(Arc::new(ArrowIpcWriteFn));
        fns.push(Arc::new(ArrowSchemaExtractFn));
        fns.push(Arc::new(ParquetPartitionWriteFn));
        fns.push(Arc::new(ParquetStatisticsFn));
        fns.push(Arc::new(ColumnarToRowFn));
    }

    #[cfg(feature = "format-image")]
    {
        use crate::builtins_format_image::*;
        fns.push(Arc::new(ImageMetadataFn));
        fns.push(Arc::new(ImageResizeFn));
        fns.push(Arc::new(ImageCropFn));
        fns.push(Arc::new(ImageRotateFn));
        fns.push(Arc::new(ImageConvertFn));
        fns.push(Arc::new(ImageThumbnailFn));
        fns.push(Arc::new(ImageStripMetadataFn));
        fns.push(Arc::new(ImageToGrayscaleFn));
        fns.push(Arc::new(ImageWatermarkFn));
        fns.push(Arc::new(ImageHashFn));
        fns.push(Arc::new(PngOptimizeFn));
        fns.push(Arc::new(JpegOptimizeFn));
        fns.push(Arc::new(SvgMinifyFn));
        fns.push(Arc::new(SvgToPngFn));
        fns.push(Arc::new(TiffSplitFn));
        fns.push(Arc::new(TiffMergeFn));
        fns.push(Arc::new(GifExtractFramesFn));
        fns.push(Arc::new(ImageDetectFormatFn));
    }

    #[cfg(feature = "format-document")]
    {
        use crate::builtins_format_document::*;
        fns.push(Arc::new(PdfMetadataFn));
        fns.push(Arc::new(PdfExtractTextFn));
        fns.push(Arc::new(PdfExtractImagesFn));
        fns.push(Arc::new(PdfMergeFn));
        fns.push(Arc::new(PdfSplitFn));
        fns.push(Arc::new(PdfPageCountFn));
        fns.push(Arc::new(PdfToTextFn));
        fns.push(Arc::new(DocxExtractTextFn));
        fns.push(Arc::new(DocxMetadataFn));
        fns.push(Arc::new(XlsxReadFn));
        fns.push(Arc::new(XlsxSheetListFn));
        fns.push(Arc::new(XlsxToCsvFn));
        fns.push(Arc::new(PptxExtractTextFn));
        fns.push(Arc::new(PptxSlideCountFn));
        fns.push(Arc::new(HtmlToTextFn));
        fns.push(Arc::new(HtmlExtractLinksFn));
        fns.push(Arc::new(HtmlExtractImagesFn));
        fns.push(Arc::new(HtmlMinifyFn));
        fns.push(Arc::new(MarkdownToHtmlFn));
        fns.push(Arc::new(HtmlToMarkdownFn));
        fns.push(Arc::new(LatexToTextFn));
        fns.push(Arc::new(EpubExtractTextFn));
        fns.push(Arc::new(EpubMetadataFn));
    }

    #[cfg(feature = "format-audio")]
    {
        use crate::builtins_format_audio::*;
        fns.push(Arc::new(AudioMetadataFn));
        fns.push(Arc::new(AudioConvertFn));
        fns.push(Arc::new(AudioTrimFn));
        fns.push(Arc::new(AudioNormalizeFn));
        fns.push(Arc::new(AudioWaveformFn));
        fns.push(Arc::new(AudioSilenceDetectFn));
        fns.push(Arc::new(AudioStripMetadataFn));
        fns.push(Arc::new(VideoMetadataFn));
        fns.push(Arc::new(VideoThumbnailFn));
        fns.push(Arc::new(VideoExtractAudioFn));
        fns.push(Arc::new(VideoStripAudioFn));
        fns.push(Arc::new(VideoResolutionFn));
        fns.push(Arc::new(SubtitleExtractFn));
        fns.push(Arc::new(WavToRawPcmFn));
        fns.push(Arc::new(RawPcmToWavFn));
        fns.push(Arc::new(MediaDetectFormatFn));
    }

    #[cfg(feature = "format-geo")]
    {
        use crate::builtins_format_geo::*;
        fns.push(Arc::new(GeoJsonValidateFn));
        fns.push(Arc::new(GeoJsonFilterFn));
        fns.push(Arc::new(GeoJsonBboxFn));
        fns.push(Arc::new(GeoJsonSimplifyFn));
        fns.push(Arc::new(GeoJsonToWktFn));
        fns.push(Arc::new(WktToGeoJsonFn));
        fns.push(Arc::new(ShapefileToGeoJsonFn));
        fns.push(Arc::new(KmlToGeoJsonFn));
        fns.push(Arc::new(GpxToGeoJsonFn));
        fns.push(Arc::new(GeoTiffMetadataFn));
        fns.push(Arc::new(GeoTiffCropFn));
        fns.push(Arc::new(GeoTiffResampleFn));
        fns.push(Arc::new(FlatGeobufReadFn));
        fns.push(Arc::new(FlatGeobufWriteFn));
    }

    #[cfg(feature = "format-scientific")]
    {
        use crate::builtins_format_scientific::*;
        fns.push(Arc::new(Hdf5MetadataFn));
        fns.push(Arc::new(Hdf5ReadFn));
        fns.push(Arc::new(Hdf5WriteFn));
        fns.push(Arc::new(NetcdfMetadataFn));
        fns.push(Arc::new(NetcdfReadFn));
        fns.push(Arc::new(FitsMetadataFn));
        fns.push(Arc::new(FitsReadFn));
        fns.push(Arc::new(NumpyReadFn));
        fns.push(Arc::new(NumpyWriteFn));
        fns.push(Arc::new(ZarrReadFn));
        fns.push(Arc::new(ZarrMetadataFn));
        fns.push(Arc::new(NumpyToArrowFn));
        fns.push(Arc::new(ArrowToNumpyFn));
    }

    #[cfg(feature = "format-database")]
    {
        use crate::builtins_format_database::*;
        fns.push(Arc::new(SqliteQueryFn));
        fns.push(Arc::new(SqliteTableListFn));
        fns.push(Arc::new(SqliteToCsvFn));
        fns.push(Arc::new(CsvToSqliteFn));
        fns.push(Arc::new(PostgresCopyParseFn));
        fns.push(Arc::new(PostgresCopyWriteFn));
        fns.push(Arc::new(SstMetadataFn));
        fns.push(Arc::new(SstScanFn));
        fns.push(Arc::new(WalParseFn));
        fns.push(Arc::new(SqlDumpParseFn));
    }

    #[cfg(feature = "format-ml")]
    {
        use crate::builtins_format_ml::*;
        fns.push(Arc::new(TfRecordReadFn));
        fns.push(Arc::new(TfRecordWriteFn));
        fns.push(Arc::new(SafeTensorsReadFn));
        fns.push(Arc::new(SafeTensorsMetadataFn));
        fns.push(Arc::new(SafeTensorsWriteFn));
        fns.push(Arc::new(OnnxMetadataFn));
        fns.push(Arc::new(OnnxValidateFn));
        fns.push(Arc::new(GgufMetadataFn));
        fns.push(Arc::new(PickleToJsonFn));
        fns.push(Arc::new(NumpyToSafeTensorsFn));
        fns.push(Arc::new(TfRecordToParquetFn));
        fns.push(Arc::new(ImageToTensorFn));
    }

    #[cfg(feature = "format-network")]
    {
        use crate::builtins_format_network::*;
        fns.push(Arc::new(PcapReadFn));
        fns.push(Arc::new(PcapFilterFn));
        fns.push(Arc::new(PcapStatisticsFn));
        fns.push(Arc::new(DnsRecordParseFn));
        fns.push(Arc::new(HarParseFn));
        fns.push(Arc::new(EmailParseFn));
        fns.push(Arc::new(EmailExtractAttachmentsFn));
        fns.push(Arc::new(MboxSplitFn));
    }

    #[cfg(feature = "format-bio")]
    {
        use crate::builtins_format_bio::*;
        fns.push(Arc::new(FastaParseFn));
        fns.push(Arc::new(FastqParseFn));
        fns.push(Arc::new(FastqTrimQualityFn));
        fns.push(Arc::new(FastqFilterFn));
        fns.push(Arc::new(SamParseFn));
        fns.push(Arc::new(BamReadFn));
        fns.push(Arc::new(BamToSamFn));
        fns.push(Arc::new(VcfParseFn));
        fns.push(Arc::new(VcfFilterFn));
        fns.push(Arc::new(BedParseFn));
        fns.push(Arc::new(GffParseFn));
        fns.push(Arc::new(FastaToFastqFn));
    }

    #[cfg(feature = "format-binary")]
    {
        use crate::builtins_format_binary::*;
        fns.push(Arc::new(FontMetadataFn));
        fns.push(Arc::new(WoffToOtfFn));
        fns.push(Arc::new(OtfToWoff2Fn));
        fns.push(Arc::new(GltfMetadataFn));
        fns.push(Arc::new(GltfValidateFn));
        fns.push(Arc::new(StlReadFn));
        fns.push(Arc::new(StlConvertFn));
        fns.push(Arc::new(DicomMetadataFn));
        fns.push(Arc::new(DicomToImageFn));
        fns.push(Arc::new(DicomAnonymizeFn));
        fns.push(Arc::new(WasmValidateFn));
        fns.push(Arc::new(WasmMetadataFn));
        fns.push(Arc::new(ElfMetadataFn));
    }

    fns
}
