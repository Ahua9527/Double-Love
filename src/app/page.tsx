'use client'

import { 
  Pane, 
  FileUploader, 
  FileCard, 
  Alert, 
  Heading, 
  Button,
  TextInput,
  Label,
  majorScale,
  MimeType
} from 'evergreen-ui'
import React, { useEffect, useState, useCallback } from 'react'
import { processXML } from '@/utils/xml'

// 定义错误类型
type ProcessError = {
  type: 'upload' | 'process' | 'download';
  message: string;
  fileName: string;
}

export default function Home() {
  // ===== 状态定义 =====
  // 组件挂载状态
  const [isMounted, setIsMounted] = useState(false)
  // 文件列表
  const [files, setFiles] = useState<File[]>([])
  // 文件拒绝列表
  const [fileRejections, setFileRejections] = useState<{ file: File; message: string }[]>([])
  // 处理状态
  const [isProcessing, setIsProcessing] = useState(false)
  // 错误信息
  const [errors, setErrors] = useState<ProcessError[]>([])
  // 分辨率设置
  const [width, setWidth] = useState('1920')
  const [height, setHeight] = useState('1080')
  // 自定义前缀
  const [prefix, setPrefix] = useState('')

  // ===== 辅助函数 =====
  // 验证分辨率输入
  const validateResolution = useCallback((value: string): boolean => {
    const num = parseInt(value)
    return !isNaN(num) && num > 0 && num <= 8192
  }, [])

  // 添加错误信息
  const addError = useCallback((error: ProcessError) => {
    setErrors(prev => [...prev, error])
  }, [])

  // ===== 生命周期 =====
  useEffect(() => {
    setIsMounted(true)
    // 组件卸载时清理文件对象
    return () => {
      files.forEach(file => {
        URL.revokeObjectURL(URL.createObjectURL(file))
      })
      setIsMounted(false)
    }
  }, [files])

  // 服务端渲染保护
  if (typeof window === 'undefined' || !isMounted) {
    return null
  }

  // ===== 事件处理函数 =====
  // 处理文件接受
  const handleAccepted = (acceptedFiles: File[]) => {
    setFiles(prev => [...prev, ...acceptedFiles])
    setErrors([])
  }

  // 处理文件拒绝
  const handleRejected = (rejectedFiles: { file: File; message: string }[]) => {
    setFileRejections(rejectedFiles)
    rejectedFiles.forEach(rejection => {
      addError({
        type: 'upload',
        fileName: rejection.file.name,
        message: rejection.message
      })
    })
  }

  // 处理文件移除
  const handleRemove = (file: File) => {
    setFiles(files.filter(f => f !== file))
    setFileRejections(fileRejections.filter(r => r.file !== file))
    setErrors(errors.filter(e => e.fileName !== file.name))
  }

  // 处理分辨率输入
  const handleResolutionChange = (
    e: React.ChangeEvent<HTMLInputElement>,
    setter: (value: string) => void
  ) => {
    const value = e.target.value
    if (value === '' || validateResolution(value)) {
      setter(value)
    }
  }

  // 处理文件处理
  const handleProcess = async () => {
    setIsProcessing(true)
    setErrors([])
    
    try {
      // 串行处理所有文件
      const successfulFiles: File[] = [];
      
      for (const file of files) {
        try {
          // 处理 XML
          const processedXML = await processXML(file, {
            width: parseInt(width),
            height: parseInt(height),
            format: '{scene}_{shot}_{take}{camera}_{Rating}',
            prefix
          });

          // 创建下载
          const blob = new Blob([processedXML], { type: 'text/xml;charset=utf-8' });
          const url = URL.createObjectURL(blob);
          
          // 下载延迟
          await new Promise(resolve => setTimeout(resolve, 500));
          
          // 触发下载
          const a = document.createElement('a');
          a.href = url;
          a.download = file.name.replace('.xml', '_Double_LOVE.xml');
          document.body.appendChild(a);
          a.click();
          
          // 清理
          await new Promise(resolve => setTimeout(resolve, 500));
          document.body.removeChild(a);
          URL.revokeObjectURL(url);
          
          successfulFiles.push(file);
        } catch (err) {
          addError({
            type: 'process',
            fileName: file.name,
            message: err instanceof Error ? err.message : '处理失败'
          });
        }
      }

      // 移除处理成功的文件
      setFiles(prev => prev.filter(f => !successfulFiles.includes(f)));
      
    } catch (err) {
      addError({
        type: 'process',
        fileName: 'batch',
        message: err instanceof Error ? err.message : '批量处理失败'
      })
    } finally {
      setIsProcessing(false)
    }
  }

  // ===== 渲染错误提示 =====
  const renderErrors = () => {
    const errorsByType = {
      upload: errors.filter(e => e.type === 'upload'),
      process: errors.filter(e => e.type === 'process'),
      download: errors.filter(e => e.type === 'download')
    }

    return Object.entries(errorsByType).map(([type, typeErrors]) => {
      if (typeErrors.length === 0) return null
      return (
        <Alert
          key={type}
          intent="danger"
          title={`${type === 'upload' ? '上传' : type === 'process' ? '处理' : '下载'}错误`}
          marginBottom={majorScale(2)}
        >
          {typeErrors.map(err => (
            <div key={err.fileName}>{err.fileName}: {err.message}</div>
          ))}
        </Alert>
      )
    })
  }

  // ===== 组件渲染 =====
  return (
    <Pane 
      display="flex" 
      alignItems="center" 
      justifyContent="center" 
      minHeight="100vh"
      padding={16}
      backgroundColor="white"
    >
      <Pane 
        elevation={1} 
        backgroundColor="white"
        padding={32} 
        borderRadius={8}
        width="100%"
        maxWidth={600}
      >
        {/* 标题 */}
        <Heading 
          size={900} 
          marginBottom={majorScale(4)} 
          textAlign="center"
        >
          Double-LOVE
        </Heading>

        {/* 错误提示 */}
        {renderErrors()}

        {/* 自定义前缀输入 */}
        <Pane marginBottom={majorScale(3)}>
          <Label htmlFor="prefix" marginBottom={majorScale(1)}>
            自定义前缀（可选）
          </Label>
          <TextInput
            id="prefix"
            value={prefix}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) => setPrefix(e.target.value)}
            width="100%"
            placeholder="输入自定义前缀"
          />
        </Pane>

        {/* 分辨率设置 */}
        <Pane display="flex" gap={majorScale(1)} marginBottom={majorScale(3)}>
          <Pane flex={1}>
            <Label htmlFor="width" marginBottom={majorScale(1)}>
              项目分辨率 - 宽度
            </Label>
            <TextInput
              id="width"
              value={width}
              onChange={(e) => handleResolutionChange(e, setWidth)}
              isInvalid={width !== '' && !validateResolution(width)}
              width="100%"
              placeholder="输入宽度"
            />
            {width !== '' && !validateResolution(width) && (
              <Alert
                intent="danger"
                marginTop={majorScale(1)}
                title="请输入有效的分辨率（1-8192）"
              />
            )}
          </Pane>
          <Pane display="flex" alignItems="center" marginX={majorScale(0)} paddingX={0} marginY={majorScale(3)}>
            x
          </Pane>
          <Pane flex={1}>
            <Label htmlFor="height" marginBottom={majorScale(1)}>
              项目分辨率 - 高度
            </Label>
            <TextInput
              id="height"
              value={height}
              onChange={(e) => handleResolutionChange(e, setHeight)}
              isInvalid={height !== '' && !validateResolution(height)}
              width="100%"
              placeholder="输入高度"
            />
            {height !== '' && !validateResolution(height) && (
              <Alert
                intent="danger"
                marginTop={majorScale(1)}
                title="请输入有效的分辨率（1-8192）"
              />
            )}
          </Pane>
        </Pane>

        {/* 文件上传区域 */}
        <FileUploader
          label="上传 XML 文件"
          description="将文件拖放到此处或点击选择文件。支持上传多个文件。"
          maxSizeInBytes={50 * 1024 ** 2}
          acceptedMimeTypes={[
            'text/xml' as MimeType
          ]}
          onAccepted={handleAccepted}
          onRejected={handleRejected}
          disabled={isProcessing}
          maxFiles={10}
          marginBottom={majorScale(3)}
        />
        
        {/* 已上传文件列表 */}
        {files.length > 0 && (
          <Pane marginY={majorScale(3)}>
            <Heading size={400} marginBottom={majorScale(2)}>
              已上传文件
            </Heading>
            {files.map((file, index) => (
              <FileCard
                key={`${file.name}-${index}`}
                name={file.name}
                sizeInBytes={file.size}
                onRemove={() => handleRemove(file)}
                marginBottom={majorScale(1)}
              />
            ))}
          </Pane>
        )}

        {/* 处理按钮 */}
        {files.length > 0 && (
          <Button
            appearance="primary"
            intent="primary"
            onClick={handleProcess}
            isLoading={isProcessing}
            disabled={isProcessing}
            width="100%"
            height={40}
            aria-label={`处理 ${files.length} 个文件`}
          >
            {isProcessing ? `正在处理 ${files.length} 个文件...` : `处理 ${files.length} 个文件`}
          </Button>
        )}
      </Pane>
    </Pane>
  )
}
