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
import { processXML } from '../utils/xml';
import { getVersionDisplay } from '../config/version';
/**
 * Double LOVE文件上传组件
 * @returns {JSX.Element} 文件上传处理界面
 */
const DoubleLoveUploader = () => {
  // 组件状态管理
  const [prefix, setPrefix] = useState(''); // 文件前缀
  const [width, setWidth] = useState('1920'); // 默认宽度
  const [height, setHeight] = useState('1080'); // 默认高度
  const [files, setFiles] = useState<File[]>([]); // 已上传文件列表
  const [isDragging, setIsDragging] = useState(false); // 拖拽状态
  const [processing, setProcessing] = useState(false); // 处理中状态
  const [currentFile, setCurrentFile] = useState<string>(''); // 当前处理文件
  const [progress, setProgress] = useState<number>(0); // 处理进度
  const fileInputRef = useRef<HTMLInputElement>(null); // 文件输入引用

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

  /**
   * 处理文件上传
   * @param {File[]} newFiles - 新上传的文件列表
   */
  const handleFiles = (newFiles: File[]) => {
        if (files.length + newFiles.length > 99) {
      alert('最多只能上传99个文件');
      return;
    }

    const validFiles = newFiles.filter(file => {
      const isXML = file.name.toLowerCase().endsWith('.xml');
      const isValidSize = file.size <= 50 * 1024 * 1024;       return isXML && isValidSize;
    });

    if (validFiles.length === 0) {
      alert('请上传XML文件，且文件大小不超过50MB');
      return;
    }

        setFiles(prevFiles => [...prevFiles, ...validFiles]);
  };

  /**
   * 移除单个文件
   * @param {number} index - 要移除的文件索引
   */
  const removeFile = (index: number) => {
    setFiles(prev => prev.filter((_, i) => i !== index));
  };

  /**
   * 清空所有文件
   */
  const clearFiles = () => {
    setFiles([]);
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
    if (!files.length) return;
    
    setProcessing(true);
    setProgress(0);
    
    try {
      for (let i = 0; i < files.length; i++) {
        const file = files[i];
        setCurrentFile(file.name);
        setProgress((i / files.length) * 100);

        const processedXML = await processXML(file, {
          prefix,
          width: parseInt(width),
          height: parseInt(height),
          onProgress: (percent) => {
            setProgress(((i + percent / 100) / files.length) * 100);
          }
        });

        const originalName = file.name;
        const extension = originalName.toLowerCase().endsWith('.xml') ? '' : '.xml';
        const newFileName = `${originalName.replace('.xml', '')}_Double_LOVE${extension}`;

        const blob = new Blob([processedXML], { type: 'text/xml' });
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = newFileName;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);

        await new Promise(resolve => setTimeout(resolve, 1000));
      }

      setProgress(100);
    } catch (error) {
      console.error('Processing error:', error);
      alert('处理文件时发生错误，请检查文件格式是否正确');
    } finally {
      setProcessing(false);
      setCurrentFile('');
      setProgress(0);
    }
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
          {/* 自定义前缀输入 */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2 transition-colors duration-500 ease-in-out">
              自定义前缀
            </label>
            <input
              type="text"
              placeholder="输入自定义前缀（可选）"
              value={prefix}
              onChange={(e) => setPrefix(e.target.value)}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 
                         bg-light-input dark:bg-dark-input text-gray-900 dark:text-white 
                         rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent
                         transition-all duration-500 ease-in-out"
            />
          </div>

          {/* 分辨率设置 */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-dark-placeholder mb-2 transition-colors duration-500 ease-in-out">
              分辨率
            </label>
            <div className="flex items-center space-x-2">
              <div className="flex-1">
                <input
                  type="text"
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
                  type="text"
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
          </div>

          {/* 文件上传区域 */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              上传 XML 文件
            </label>
            <div
              className={`border-2 border-dashed rounded-xl p-8 transition-all cursor-pointer
                ${isDragging 
                  ? 'border-selected bg-cyan-50 dark:bg-cyan-900' 
                  : 'border-gray-300 dark:border-gray-600 hover:bg-light-bg dark:hover:bg-dark-bg'
                }`}
              onDragEnter={handleDragEnter}
              onDragLeave={handleDragLeave}
              onDragOver={handleDragOver}
              onDrop={handleDrop}
              onClick={() => fileInputRef.current?.click()}
            >
              <input
                type="file"
                className="hidden"
                ref={fileInputRef}
                accept=".xml"
                multiple
                onChange={(e) => handleFiles(Array.from(e.target.files || []))}
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
                  <p className={`text-xs transition-colors mt-1 ${
                    isDragging 
                      ? 'text-selected/80 dark:text-selected/70' 
                      : 'text-gray-500 dark:text-gray-400'
                  }`}>
                  </p>
                </div>
              </div>
            </div>
          </div>

          {/* 文件列表 */}
          {files.length > 0 && (
            <div className="space-y-4">
              <div className="flex justify-between items-center">
                <h3 className="text-sm font-medium text-gray-700 dark:text-gray-300">
                  已上传文件 ({files.length})
                </h3>
                <button
                  onClick={clearFiles}
                  className="text-sm text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
                >
                  清空
                </button>
              </div>
              {files.map((file, index) => (
                <div
                  key={index}
                  className="group flex items-center justify-between p-3 bg-white dark:bg-gray-700 
                           border border-gray-200 dark:border-gray-600 rounded-lg shadow-sm
                           hover:bg-gray-50 dark:hover:bg-gray-600 transition-colors"
                >
                  <div className="flex items-center space-x-3 min-w-0">
                    <FileText className="flex-shrink-0 w-5 h-5 text-gray-400 dark:text-gray-500" />
                    <div className="min-w-0">
                      <p className="text-sm font-medium text-gray-700 dark:text-gray-300 truncate">
                        {file.name}
                      </p>
                      <p className="text-xs text-gray-500 dark:text-gray-400">
                        {formatFileSize(file.size)}
                      </p>
                    </div>
                  </div>
                  <button
                    onClick={() => removeFile(index)}
                    className="p-1 text-gray-400 dark:text-gray-500 hover:text-red-500 
                              dark:hover:text-red-400 rounded-full transition-colors"
                    title="移除文件"
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
              <div className="w-full bg-gray-200 dark:bg-gray-600 rounded-full h-2.5 relative overflow-hidden">
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
              ) : `处理 ${files.length} 个文件`}
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