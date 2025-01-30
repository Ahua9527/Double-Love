import { useState, useRef } from 'react';
import { Upload, FileText, X ,Github} from 'lucide-react';
import { processXML } from '../utils/xml';


const DoubleLoveUploader = () => {
  const [prefix, setPrefix] = useState('');
  const [width, setWidth] = useState('1920');
  const [height, setHeight] = useState('1080');
  const [files, setFiles] = useState<File[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [processing, setProcessing] = useState(false);
  const [currentFile, setCurrentFile] = useState<string>('');
  const [progress, setProgress] = useState<number>(0);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleDragEnter = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(true);
  };

  const handleDragLeave = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
  };

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragging(false);
    
    const droppedFiles = Array.from(e.dataTransfer.files);
    handleFiles(droppedFiles);
  };

  const handleFiles = (newFiles: File[]) => {
    // 检查文件总数是否超过限制
    if (files.length + newFiles.length > 99) {
      alert('最多只能上传99个文件');
      return;
    }

    const validFiles = newFiles.filter(file => {
      const isXML = file.name.toLowerCase().endsWith('.xml');
      const isValidSize = file.size <= 50 * 1024 * 1024; // 50MB
      return isXML && isValidSize;
    });

    if (validFiles.length === 0) {
      alert('请上传XML文件，且文件大小不超过50MB');
      return;
    }

    // 合并当前文件和新文件
    setFiles(prevFiles => [...prevFiles, ...validFiles]);
  };

  const removeFile = (index: number) => {
    setFiles(prev => prev.filter((_, i) => i !== index));
  };

  const clearFiles = () => {
    setFiles([]);
  };

  const formatFileSize = (bytes: number): string => {
    if (bytes < 1024) return bytes + ' B';
    if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
    return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
  };

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

  return (
    // {/* 主容器 */}
    <div className="min-h-screen flex flex-col bg-light-bg dark:bg-dark-bg transition-all duration-500 ease-in-out">
      {/* 内容区域 */}
      <main className="flex-grow flex items-center justify-center p-6 pb-32 bg-light-bg dark:bg-dark-bg">
        <div className="w-full max-w-2xl bg-light-card dark:bg-dark-card rounded-2xl shadow-xl p-10 min-h-[600px] transition-all duration-500 ease-in-out">
        <h1 className="text-3xl font-chalkboard font-bold text-gray-900 dark:text-white mt-6 mb-12 text-center tracking-wide transition-colors duration-500 ease-in-out">
          Double<span className="text-selected">-</span>LOVE
        </h1>
          

        
        
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

          {/* 分辨率输入区域 */}
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
              <div className="text-center">
                <Upload className="mx-auto h-12 w-12 text-gray-400 dark:text-gray-500" />
                <p className="mt-2 text-sm text-gray-500 dark:text-gray-400">
                  支持拖放或点击上传，最多99个文件
                </p>
                <p className="mt-1 text-sm text-blue-500 hover:text-blue-500">
                  点击或拖拽文件到此处
                </p>
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
                  className="flex items-center justify-between p-3 bg-white dark:bg-gray-700 
                           border border-gray-200 dark:border-gray-600 rounded-lg shadow-sm"
                >
                  <div className="flex items-center space-x-3">
                    <FileText className="w-5 h-5 text-gray-400 dark:text-gray-500" />
                    <div>
                      <p className="text-sm font-medium text-gray-700 dark:text-gray-300">{file.name}</p>
                      <p className="text-xs text-gray-500 dark:text-gray-400">{formatFileSize(file.size)}</p>
                    </div>
                  </div>
                  <button
                    onClick={() => removeFile(index)}
                    className="p-1 hover:bg-gray-100 dark:hover:bg-gray-600 rounded-full"
                  >
                    <X className="w-4 h-4 text-gray-500 dark:text-gray-400" />
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
              <div className="w-full bg-gray-200 dark:bg-gray-600 rounded-full h-2">
                <div 
                  className="bg-selected h-2 rounded-full transition-all duration-300"
                  style={{ width: `${progress}%` }}
                />
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
              {processing ? '处理中...' : `处理 ${files.length} 个文件`}
            </button>
          )}
        </div>
        </div>
      </main>
      {/* 固定底部 */}
      <footer className="fixed bottom-0 w-full bg-gradient-to-t from-light-bg/95 via-light-bg/80 to-light-bg/0 dark:from-dark-bg/95 dark:via-dark-bg/80 dark:to-dark-bg/0">
        <div className="container mx-auto px-4 py-4">
          <div className="flex items-center justify-center space-x-6">
            <a
              href="https://github.com/Ahua9527/Double-Love-Web"
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
      {/* 背景遮罩层 */}
      <div className="fixed inset-0 -z-10 bg-light-bg dark:bg-dark-bg"></div>
    </div>
    
  );
};

export default DoubleLoveUploader;