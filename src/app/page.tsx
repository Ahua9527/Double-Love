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
import React, { useEffect, useState } from 'react'
import { processXML } from '@/utils/xml'

export default function Home() {
  const [isMounted, setIsMounted] = useState(false)
  const [files, setFiles] = useState<File[]>([])
  const [fileRejections, setFileRejections] = useState<{ file: File; message: string }[]>([])
  const [isProcessing, setIsProcessing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [width, setWidth] = useState('1920')
  const [height, setHeight] = useState('1080')
  const [prefix, setPrefix] = useState('') // 新增前缀状态

  useEffect(() => {
    setIsMounted(true)
    return () => setIsMounted(false)
  }, [])

  if (typeof window === 'undefined' || !isMounted) {
    return null
  }

  const handleAccepted = (acceptedFiles: File[]) => {
    setFiles(prev => [...prev, ...acceptedFiles])
    setError(null)
  }

  const handleRejected = (rejectedFiles: { file: File; message: string }[]) => {
    setFileRejections(rejectedFiles)
    const rejectionMessages = rejectedFiles.map(r => 
      `文件 ${r.file.name} 无法上传：${r.message}`
    ).join('\n')
    setError(rejectionMessages)
  }

  const handleRemove = (file: File) => {
    setFiles(files.filter(f => f !== file))
    setFileRejections(fileRejections.filter(r => r.file !== file))
    setError(null)
  }

  const handleProcess = async () => {
    setIsProcessing(true)
    setError(null)
    try {
      const successfulFiles: File[] = []
      
      for (let i = 0; i < files.length; i++) {
        const file = files[i]
        try {
          const processedXML = await processXML(file, {
            width: parseInt(width),
            height: parseInt(height),
            format: '{scene}_{shot}_{take}{camera}_{Rating}',
            prefix: prefix // 传递前缀
          })
          const blob = new Blob([processedXML], { type: 'text/xml;charset=utf-8' })
          const url = URL.createObjectURL(blob)
          
          await new Promise(resolve => setTimeout(resolve, 500))
          
          const a = document.createElement('a')
          a.href = url
          a.download = file.name.replace('.xml', '_Double_LOVE.xml')
          document.body.appendChild(a)
          a.click()
          
          await new Promise(resolve => setTimeout(resolve, 500))
          
          document.body.removeChild(a)
          URL.revokeObjectURL(url)
          successfulFiles.push(file)
        } catch (err) {
          console.error(`处理文件 ${file.name} 时出错:`, err)
          setError(`文件 ${file.name} 处理失败，已跳过`)
          continue
        }
      }

      setFiles(prev => prev.filter(f => !successfulFiles.includes(f)))
      setFileRejections([])
      
      if (successfulFiles.length > 0) {
        setError(null)
      }
    } catch (err: unknown) {
      const errorMessage = err instanceof Error ? err.message : '处理文件时出错'
      console.error('处理文件时出错:', err)
      setError(errorMessage)
    } finally {
      setIsProcessing(false)
    }
  }

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
        <Heading 
          size={900} 
          marginBottom={majorScale(4)} 
          textAlign="center"
        >
          Double-Love
        </Heading>

        {error && (
          <Alert 
            intent="danger" 
            title="错误" 
            marginBottom={majorScale(3)}
          >
            {error}
          </Alert>
        )}

        {/* 新增前缀输入框 */}
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

        <Pane display="flex" gap={majorScale(1)} marginBottom={majorScale(3)}>
          <Pane flex={1}>
            <Label htmlFor="width" marginBottom={majorScale(1)}>
              项目分辨率 - 宽度
            </Label>
            <TextInput
              id="width"
              value={width}
              onChange={(e: React.ChangeEvent<HTMLInputElement>) => setWidth(e.target.value)}
              width="100%"
              placeholder="输入宽度"
            />
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
              onChange={(e: React.ChangeEvent<HTMLInputElement>) => setHeight(e.target.value)}
              width="100%"
              placeholder="输入高度"
            />
          </Pane>
        </Pane>

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
                marginBottom={majorScale(0)}
              />
            ))}
          </Pane>
        )}

        {fileRejections.map((rejection, index) => (
          <Alert
            key={`${rejection.file.name}-${index}`}
            intent="danger"
            title={`文件 ${rejection.file.name} 无法上传`}
            marginBottom={majorScale(2)}
          >
            {rejection.message}
          </Alert>
        ))}

        {files.length > 0 && (
          <Button
            appearance="primary"
            intent="primary"
            onClick={handleProcess}
            isLoading={isProcessing}
            disabled={isProcessing}
            width="100%"
            height={40}
          >
            {isProcessing ? `正在处理 ${files.length} 个文件...` : `处理 ${files.length} 个文件`}
          </Button>
        )}
      </Pane>
    </Pane>
  )
}
