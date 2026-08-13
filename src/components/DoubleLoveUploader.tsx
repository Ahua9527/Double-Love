/**
 * Double LOVE文件上传处理组件
 * 
 * 主要功能：
 * 1. 支持拖拽和点击上传XML文件
 * 2. 文件格式和大小验证
 * 3. 批量文件处理与进度跟踪
 * 4. 自定义前缀和分辨率设置
 * 5. 处理结果文件下载
 */
import { useState, useRef } from 'react';
import { Upload, FileText, X, Github } from 'lucide-react';
import {
  processXML,
  parseCSVForSeasonEpisode,
  isStrictPositiveInteger,
  type XMLDiagnostic,
  type XMLProcessResult,
} from '../utils/xml';
import { getOutputFileName } from '../utils/download';
import { getVersionDisplay } from '../config/version';

interface FileProcessingReport {
  fileName: string;
  result: XMLProcessResult;
}

function downloadXML(xml: string, fileName: string): void {
  const blob = new Blob([xml], { type: 'text/xml' });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = fileName;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

function getStatusLabel(status: XMLProcessResult['status']): string {
  if (status === 'success') return '成功';
  if (status === 'partial') return '部分完成';
  return '失败';
}
/**
 * Double LOVE文件上传组件
 * @returns {JSX.Element} 文件上传处理界面
 */
const DoubleLoveUploader = () => {
  // 组件状态管理
  const [width, setWidth] = useState('1920'); // 默认宽度
  const [height, setHeight] = useState('1080'); // 默认高度
  const [files, setFiles] = useState<File[]>([]); // 已上传文件列表
  const [csvFiles, setCsvFiles] = useState<File[]>([]); // CSV文件列表
  const [isDragging, setIsDragging] = useState(false); // 拖拽状态
  const [processing, setProcessing] = useState(false); // 处理中状态
  const [currentFile, setCurrentFile] = useState<string>(''); // 当前处理文件
  const [progress, setProgress] = useState<number>(0); // 处理进度
  const [inputError, setInputError] = useState('');
  const [csvDiagnostics, setCsvDiagnostics] = useState<XMLDiagnostic[]>([]);
  const [reports, setReports] = useState<FileProcessingReport[]>([]);
  const [usesCsvNaming, setUsesCsvNaming] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null); // 文件输入引用
  const fileKeysRef = useRef(new WeakMap<File, string>());
  const nextFileKeyRef = useRef(0);

  const getStableFileKey = (file: File): string => {
    const existingKey = fileKeysRef.current.get(file);
    if (existingKey) return existingKey;

    const key = `upload-${nextFileKeyRef.current}`;
    nextFileKeyRef.current += 1;
    fileKeysRef.current.set(file, key);
    return key;
  };

  const getFileSignature = (file: File): string => (
    `${file.name.toLowerCase()}\u0000${file.size}\u0000${file.lastModified}\u0000${file.type}`
  );

  /**
   * 处理拖拽进入事件
   * @param {React.DragEvent} e - 拖拽事件对象
   */

  const handleDragEnter = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(true);
  };

  /**
   * 处理拖拽离开事件
   * @param {React.DragEvent} e - 拖拽事件对象
   */
  const handleDragLeave = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
  };

  /**
   * 处理拖拽悬停事件
   * @param {React.DragEvent} e - 拖拽事件对象
   */
  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };

  /**
   * 处理文件放置事件
   * @param {React.DragEvent} e - 拖拽事件对象
   */
  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
    
    const droppedFiles = Array.from(e.dataTransfer.files);
    handleFiles(droppedFiles);
  };

  const handleUploadKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      fileInputRef.current?.click();
    }
  };

  /**
   * 处理文件上传
   * @param {File[]} newFiles - 新上传的文件列表
   */
  const handleFiles = (newFiles: File[]) => {
    const inputMessages: string[] = [];
    const candidateFiles = newFiles.filter(file => {
      const lowerName = file.name.toLowerCase();
      if (lowerName.endsWith('.xml')) {
        if (file.size <= 50 * 1024 * 1024) return true;
        inputMessages.push(`${file.name}：XML 超过 50MB，已忽略。`);
        return false;
      }
      if (lowerName.endsWith('.csv')) {
        if (file.size <= 10 * 1024 * 1024) return true;
        inputMessages.push(`${file.name}：CSV 超过 10MB，已忽略。`);
        return false;
      }
      inputMessages.push(`${file.name}：不支持的文件类型，已忽略。`);
      return false;
    });

    const seenSignatures = new Set([...files, ...csvFiles].map(getFileSignature));
    const uniqueFiles = candidateFiles.filter(file => {
      const signature = getFileSignature(file);
      if (seenSignatures.has(signature)) {
        inputMessages.push(`${file.name}：重复文件，已忽略。`);
        return false;
      }
      seenSignatures.add(signature);
      return true;
    });
    const candidateXmlFiles = uniqueFiles.filter(file => file.name.toLowerCase().endsWith('.xml'));
    const candidateCsvFiles = uniqueFiles.filter(file => file.name.toLowerCase().endsWith('.csv'));
    const csvCapacity = csvFiles.length === 0 ? 1 : 0;
    const acceptedCsvFiles = candidateCsvFiles.slice(0, csvCapacity);
    for (const file of candidateCsvFiles.slice(csvCapacity)) {
      inputMessages.push(`${file.name}：当前只允许一个 CSV，已忽略。`);
    }

    const remainingCapacity = Math.max(0, 99 - files.length - csvFiles.length);
    const acceptedFiles = [...candidateXmlFiles, ...acceptedCsvFiles].slice(0, remainingCapacity);
    for (const file of [...candidateXmlFiles, ...acceptedCsvFiles].slice(remainingCapacity)) {
      inputMessages.push(`${file.name}：已达到 99 个文件上限，已忽略。`);
    }
    const xmlFiles = acceptedFiles.filter(file => file.name.toLowerCase().endsWith('.xml'));
    const newCsvFiles = acceptedFiles.filter(file => file.name.toLowerCase().endsWith('.csv'));

    if (xmlFiles.length === 0 && newCsvFiles.length === 0) {
      setInputError(inputMessages.join(' ') || '请上传 XML 或 CSV 文件，XML 最大 50MB，CSV 最大 10MB。');
      return;
    }

    setInputError(inputMessages.join(' '));

    if (xmlFiles.length > 0) {
      setFiles(prevFiles => [...prevFiles, ...xmlFiles]);
    }
    
    if (newCsvFiles.length > 0) {
      setCsvFiles(prevFiles => [...prevFiles, ...newCsvFiles]);
    }
  };

  /**
   * 移除单个XML文件
   * @param {number} index - 要移除的文件索引
   */
  const removeFile = (index: number) => {
    setFiles(prev => prev.filter((_, i) => i !== index));
  };
  
  /**
   * 移除单个CSV文件
   * @param {number} index - 要移除的文件索引
   */
  const removeCsvFile = (index: number) => {
    setCsvFiles(prev => prev.filter((_, i) => i !== index));
  };


  /**
   * 格式化文件大小
   * @param {number} bytes - 文件字节数
   * @returns {string} 格式化后的文件大小字符串
   */
  const formatFileSize = (bytes: number): string => {
    if (bytes < 1024) return bytes + ' B';
    if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
    return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
  };

  /**
   * 处理文件处理流程
   */
  const handleProcess = async () => {
    if (!files.length || processing) return;

    setProcessing(true);
    setProgress(0);
    setInputError('');
    setReports([]);
    setUsesCsvNaming(false);

    // 解析CSV文件获取Season和Episode映射
    let csvSeasonMap: Map<string, string> | undefined;
    let csvEpisodeMap: Map<string, string> | undefined;
    if (csvFiles.length > 0) {
      try {
        // 上传契约只允许一个 CSV 文件。
        const csvContent = await csvFiles[0].text();
        const { seasonMap, episodeMap, diagnostics } = parseCSVForSeasonEpisode(csvContent);
        csvSeasonMap = seasonMap;
        csvEpisodeMap = episodeMap;
        setCsvDiagnostics(diagnostics);
        setUsesCsvNaming(seasonMap.size > 0 || episodeMap.size > 0);

        if (diagnostics.some(diagnostic => diagnostic.level === 'error')) {
          setInputError(diagnostics.map(diagnostic => diagnostic.message).join(' '));
          setProcessing(false);
          return;
        }
      } catch (error) {
        console.error('解析CSV文件失败:', error);
        setInputError('CSV 文件读取失败，未开始处理 XML。');
        setProcessing(false);
        return;
      }
    } else {
      setCsvDiagnostics([]);
      setUsesCsvNaming(false);
    }

    // 分辨率输入校验
    const w = Number(width);
    const h = Number(height);
    if (!/^[1-9]\d*$/.test(width) || !/^[1-9]\d*$/.test(height) ||
        !isStrictPositiveInteger(w) || !isStrictPositiveInteger(h)) {
      setInputError('分辨率必须是完整的正整数，例如 1920 × 1080。');
      setProcessing(false);
      return;
    }

    const outputNameCounts = new Map<string, number>();
    for (let index = 0; index < files.length; index += 1) {
      const file = files[index];
      let namingMode = '';
      if (csvSeasonMap?.size && csvEpisodeMap?.size) {
        namingMode = '（使用 Season + Episode 命名）';
      } else if (csvEpisodeMap?.size) {
        namingMode = '（使用 Episode 命名）';
      }

      setCurrentFile(`${file.name} ${namingMode}`.trim());
      setProgress((index / files.length) * 100);

      try {
        const result = await processXML(file, {
          width: w,
          height: h,
          csvSeasonMap,
          csvEpisodeMap,
          onProgress: (percent) => {
            setProgress(((index + percent / 100) / files.length) * 100);
          },
        });

        if (result.xml) {
          const baseOutputName = getOutputFileName(file.name);
          const previousCount = outputNameCounts.get(baseOutputName) || 0;
          const duplicateNumber = previousCount + 1;
          outputNameCounts.set(baseOutputName, duplicateNumber);
          downloadXML(result.xml, getOutputFileName(file.name, duplicateNumber));
        }

        setReports(previous => [...previous, { fileName: file.name, result }]);
      } catch (error) {
        const result: XMLProcessResult = {
          status: 'failed',
          counts: { total: 0, processed: 0, skipped: 0, failed: 1, csvUnmatched: 0 },
          diagnostics: [{
            level: 'error',
            code: 'UNEXPECTED_PROCESSING_ERROR',
            message: error instanceof Error ? error.message : '处理文件时发生未知错误。',
            blocksDownload: true,
          }],
        };
        setReports(previous => [...previous, { fileName: file.name, result }]);
      }
    }

    setProgress(100);
    setProcessing(false);
    setCurrentFile('');
  };

  /**
   * 渲染组件界面
   */
  return (
    <div className="min-h-screen flex flex-col bg-light-bg dark:bg-dark-bg transition-all duration-500 ease-in-out">
      {/* 主内容区域 */}
      <main className="flex-grow flex items-center justify-center p-6 pb-32 bg-light-bg dark:bg-dark-bg">
        <div className="w-full max-w-2xl bg-light-card dark:bg-dark-card rounded-2xl shadow-xl p-10 min-h-[600px] transition-all duration-500 ease-in-out">
        <h1 className="text-4xl font-chalkboard font-bold text-gray-900 dark:text-white mt-6 mb-12 text-center tracking-wide transition-colors duration-500 ease-in-out [filter:drop-shadow(2px_4px_6px_rgba(0,0,0,0.3))]">
          Double<span className="text-selected"> LOVE</span>
        </h1>
        
        {/* 主要内容区域 */}
        
        <div className="space-y-6">

          {/* 分辨率设置 */}
          <div>
            <label htmlFor="resolution-width" className="block text-sm font-medium text-gray-700 dark:text-dark-placeholder mb-2 transition-colors duration-500 ease-in-out">
              分辨率
            </label>
            <div className="flex items-center space-x-2">
              <div className="flex-1">
                <input
                  id="resolution-width"
                  type="text"
                  aria-label="视频宽度"
                  value={width}
                  onChange={(e) => setWidth(e.target.value)}
                  className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 
                             bg-light-input dark:bg-dark-input text-gray-900 dark:text-white 
                             rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent
                             transition-all duration-500 ease-in-out"
                  placeholder="宽度"
                />
              </div>
              <span className="text-gray-500 dark:text-gray-400 transition-colors duration-500 ease-in-out">×</span>
              <div className="flex-1">
                <input
                  id="resolution-height"
                  type="text"
                  aria-label="视频高度"
                  value={height}
                  onChange={(e) => setHeight(e.target.value)}
                  className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 
                             bg-light-input dark:bg-dark-input text-gray-900 dark:text-white 
                             rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent
                             transition-all duration-500 ease-in-out"
                  placeholder="高度"
                />
              </div>
            </div>
            {inputError && (
              <p className="mt-2 text-sm text-red-600 dark:text-red-400" role="alert">
                {inputError}
              </p>
            )}
          </div>

          {/* 文件上传区域 */}
          <div>
            <label htmlFor="file-upload" className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              上传 XML和CSV文件
            </label>
            <div
              className={`border-2 border-dashed rounded-xl p-8 transition-all cursor-pointer
                ${isDragging 
                  ? 'border-selected bg-cyan-50 dark:bg-cyan-900' 
                  : 'border-gray-300 dark:border-gray-600 hover:bg-light-bg dark:hover:bg-dark-bg'
                }`}
              role="button"
              tabIndex={0}
              aria-label="上传 XML 或 CSV 文件"
              onDragEnter={handleDragEnter}
              onDragLeave={handleDragLeave}
              onDragOver={handleDragOver}
              onDrop={handleDrop}
              onKeyDown={handleUploadKeyDown}
              onClick={(e) => {
                if ((e.target as HTMLElement).tagName !== 'INPUT') {
                  fileInputRef.current?.click();
                }
              }}
            >
              <input
                id="file-upload"
                type="file"
                className="hidden"
                ref={fileInputRef}
                accept=".xml,.csv"
                multiple
                aria-label="选择 XML 或 CSV 文件"
                onChange={(e) => {
                  handleFiles(Array.from(e.target.files || []));
                  e.currentTarget.value = '';
                }}
              />
              <div className="text-center space-y-2">
                <Upload className={`mx-auto h-12 w-12 transition-all duration-300 ${
                  isDragging 
                    ? 'text-selected scale-110 animate-pulse' 
                    : 'text-gray-400 dark:text-gray-500 hover:scale-105'
                }`} />
                <div className={`transition-all duration-300 ${
                  isDragging 
                    ? 'bg-selected/10 dark:bg-selected/20 rounded-lg py-2 px-4'
                    : ''
                }`}>
                  <p className={`text-sm font-medium transition-colors ${
                    isDragging 
                      ? 'text-selected' 
                      : 'text-blue-500 hover:text-blue-500'
                  }`}>
                    {isDragging ? '松开鼠标上传文件' : '点击或拖拽文件到此处'}
                  </p>
                </div>
              </div>
            </div>
          </div>

          {/* XML文件列表 */}
          {files.length > 0 && (
            <div className="space-y-4">
              <div className="flex justify-between items-center">
                <h3 className="text-sm font-medium text-gray-700 dark:text-gray-300">
                  已上传 XML 文件 ({files.length})
                </h3>
                <button
                  type="button"
                  onClick={() => setFiles([])}
                  className="text-sm text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
                >
                  清空
                </button>
              </div>
              {files.map((file, index) => (
                <div
                  key={getStableFileKey(file)}
                  className="group flex items-center justify-between p-3 bg-white dark:bg-gray-700 
                           border border-gray-200 dark:border-gray-600 rounded-lg shadow-sm
                           hover:bg-gray-50 dark:hover:bg-gray-600 transition-colors"
                >
                  <div className="flex items-center space-x-3 min-w-0">
                    <FileText className="flex-shrink-0 w-5 h-5 text-blue-500 dark:text-blue-400" />
                    <div className="min-w-0">
                      <p className="text-sm font-medium text-gray-700 dark:text-gray-300 truncate">
                        {file.name}
                      </p>
                      <p className="text-xs text-gray-500 dark:text-gray-400">
                        {formatFileSize(file.size)} • XML
                      </p>
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => removeFile(index)}
                    className="p-1 text-gray-400 dark:text-gray-500 hover:text-red-500 
                              dark:hover:text-red-400 rounded-full transition-colors"
                    title="移除文件"
                    aria-label={`移除 XML 文件 ${file.name}`}
                  >
                    <X className="w-4 h-4" />
                  </button>
                </div>
              ))}
            </div>
          )}
          
          {/* CSV文件列表 */}
          {csvFiles.length > 0 && (
            <div className="space-y-4">
              <div className="flex justify-between items-center">
                <h3 className="text-sm font-medium text-gray-700 dark:text-gray-300">
                  已上传 CSV 文件 ({csvFiles.length}) 
                  <span className="text-xs text-green-500 dark:text-green-400 ml-2">• 用于辅助元数据</span>
                </h3>
                <button
                  type="button"
                  onClick={() => setCsvFiles([])}
                  className="text-sm text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
                >
                  清空
                </button>
              </div>
              {csvFiles.map((file, index) => (
                <div
                  key={getStableFileKey(file)}
                  className="group flex items-center justify-between p-3 bg-green-50 dark:bg-green-900/20 
                           border border-green-200 dark:border-green-600 rounded-lg shadow-sm
                           hover:bg-green-100 dark:hover:bg-green-900/30 transition-colors"
                >
                  <div className="flex items-center space-x-3 min-w-0">
                    <FileText className="flex-shrink-0 w-5 h-5 text-green-500 dark:text-green-400" />
                    <div className="min-w-0">
                      <p className="text-sm font-medium text-gray-700 dark:text-gray-300 truncate">
                        {file.name}
                      </p>
                      <p className="text-xs text-gray-500 dark:text-gray-400">
                        {formatFileSize(file.size)} • CSV
                      </p>
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => removeCsvFile(index)}
                    className="p-1 text-gray-400 dark:text-gray-500 hover:text-red-500 
                              dark:hover:text-red-400 rounded-full transition-colors"
                    title="移除文件"
                    aria-label={`移除 CSV 文件 ${file.name}`}
                  >
                    <X className="w-4 h-4" />
                  </button>
                </div>
              ))}
            </div>
          )}

          {/* 处理进度 */}
          {processing && (
            <div className="space-y-2">
              <div className="text-sm text-gray-500 dark:text-gray-400">
                正在处理: {currentFile}
              </div>
              {usesCsvNaming && (
                <div className="text-xs text-green-500 dark:text-green-400">
                  ✓ 使用 CSV 数据进行 Season/Episode 命名
                </div>
              )}
              <div
                className="w-full bg-gray-200 dark:bg-gray-600 rounded-full h-2.5 relative overflow-hidden"
                role="progressbar"
                aria-label="XML 处理进度"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={Math.round(progress)}
              >
                <div 
                  className="bg-gradient-to-r from-blue-400 to-selected h-full rounded-full 
                             transition-all duration-300 ease-out"
                  style={{ width: `${progress}%` }}
                />
                {/* <div className="absolute inset-0 flex items-center justify-center">
                  <span className="text-[10px] font-medium text-white dark:text-gray-900">
                    {Math.round(progress)}%
                  </span>
                </div> */}
              </div>
            </div>
          )}

          {csvDiagnostics.length > 0 && (
            <div className="space-y-1 text-xs text-amber-700 dark:text-amber-300" role="status">
              {csvDiagnostics.map((diagnostic, index) => (
                <p key={`${diagnostic.code}-${index}`}>{diagnostic.message}</p>
              ))}
            </div>
          )}

          {reports.length > 0 && (
            <div className="space-y-2" role="status" aria-live="polite">
              <h3 className="text-sm font-medium text-gray-700 dark:text-gray-300">处理结果</h3>
              {reports.map(({ fileName, result }, index) => (
                <div
                  key={`${fileName}-${index}`}
                  className="rounded-md border border-gray-200 dark:border-gray-600 p-3 text-sm"
                >
                  <div className="flex items-center justify-between gap-3">
                    <span className="truncate text-gray-700 dark:text-gray-300">{fileName}</span>
                    <span className={
                      result.status === 'success'
                        ? 'text-green-600'
                        : result.status === 'partial'
                          ? 'text-amber-600'
                          : 'text-red-600'
                    }>
                      {getStatusLabel(result.status)}
                    </span>
                  </div>
                  <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                    共 {result.counts.total} 个 clip：处理 {result.counts.processed}，跳过 {result.counts.skipped}，失败 {result.counts.failed}，XML clip 未匹配 CSV {result.counts.csvUnmatched}
                  </p>
                  {result.diagnostics.length > 0 && (
                    <ul className="mt-2 space-y-1 text-xs text-gray-600 dark:text-gray-300">
                      {result.diagnostics.map((diagnostic, diagnosticIndex) => (
                        <li key={`${diagnostic.code}-${diagnostic.clipId || 'file'}-${diagnosticIndex}`}>
                          {diagnostic.code}{diagnostic.clipId ? ` · ${diagnostic.clipId}` : ''}：{diagnostic.message}
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              ))}
            </div>
          )}

          {/* 处理按钮 */}
          {files.length > 0 && (
            <button
              onClick={handleProcess}
              disabled={processing}
              className={`w-full py-2 px-4 rounded-md font-medium transition-all
                ${processing 
                  ? 'bg-selected/70 cursor-not-allowed' 
                  : 'bg-selected hover:bg-blue-600 text-white shadow-md hover:shadow-lg'
                }`}
            >
              {processing ? (
                <span className="inline-flex items-center">
                  <svg className="animate-spin -ml-1 mr-2 h-4 w-4 text-white" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                    <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
                    <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                  </svg>
                  处理中...
                </span>
              ) : (
                <span>
                  处理 {files.length} 个XML文件
                  {csvFiles.length > 0 && (
                    <span className="text-green-200 ml-2"></span>
                  )}
                </span>
              )}
            </button>
          )}
        </div>
        </div>
      </main>
      {/* 底部版权和版本信息 */}
      <footer className="fixed bottom-0 w-full bg-gradient-to-t from-light-bg/95 via-light-bg/80 to-light-bg/0 dark:from-dark-bg/95 dark:via-dark-bg/80 dark:to-dark-bg/0">
        <div className="container mx-auto px-4 py-4">
          {/* 版本号显示 - 独立元素固定在右下角 */}
          <div className="fixed bottom-4 right-4">
            <p className="text-xs text-gray-400 dark:text-gray-500 opacity-60">
              {getVersionDisplay()}
            </p>
          </div>
          
          <div className="flex items-center justify-center">
            <a
              href="https://github.com/Ahua9527/Double-Love"
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center space-x-2 text-gray-600 dark:text-gray-300 hover:text-selected"
            >
              <Github className="w-4 h-4" />
              <span>GitHub</span>
            </a>
          </div>
          <p className="mt-2 text-xs text-center text-gray-500 dark:text-gray-400">
            Double LOVE © 2025 | Designed & Developed by 哆啦Ahua🌱
          </p>
        </div>
      </footer>
      {/* 背景层 */}
      <div className="fixed inset-0 -z-10 bg-light-bg dark:bg-dark-bg"></div>
    </div>
    
  );
};

export default DoubleLoveUploader;
