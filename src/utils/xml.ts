/**
 * XML处理配置接口
 * 
 * @property {number} [width] - 输出视频宽度，默认1920
 * @property {number} [height] - 输出视频高度，默认1080
 * @property {string} [format] - 文件名格式模板，支持{scene}、{shot}等占位符
 * @property {string} [prefix] - 文件名前缀
 * @property {Function} [onProgress] - 进度回调函数
 */
interface XMLProcessConfig {
  width?: number;        
  height?: number;       
  format?: string;       
  prefix?: string;       
  onProgress?: (percent: number) => void; 
}

/**
 * XML处理错误类型枚举
 * 
 * @enum {string}
 * @property {string} INVALID_XML - XML文件格式无效
 * @property {string} MISSING_REQUIRED_ELEMENTS - 缺少必要元素
 * @property {string} INVALID_FORMAT - 数据格式不符合要求
 */
enum XMLProcessErrorType {
  INVALID_XML = 'INVALID_XML',                             
  MISSING_REQUIRED_ELEMENTS = 'MISSING_REQUIRED_ELEMENTS',   
  INVALID_FORMAT = 'INVALID_FORMAT'                      
}

/**
 * 自定义XML处理错误类
 * 
 * @extends Error
 * @property {XMLProcessErrorType} type - 错误类型
 */
class XMLProcessError extends Error {
  constructor(
    public type: XMLProcessErrorType,
    message: string
  ) {
    super(message);
    this.name = 'XMLProcessError';
  }
}

/**
 * XML元素集合接口
 * 
 * @interface
 * @property {Element} logginginfo - 日志信息父元素
 * @property {Element} scene - 场景编号元素
 * @property {Element} shottake - 镜头拍摄元素
 * @property {Element} filmdata - 胶片数据元素
 * @property {Element} comments - 评论父元素
 * @property {Element} mastercomment2 - 主评论元素
 * @property {Element | null} labels - 标签元素
 */
interface ClipElements {
  logginginfo: Element;        
  scene: Element;              
  shottake: Element;           
  filmdata: Element;           
  comments: Element;           
  mastercomment2: Element;
  labels: Element | null;   // 修改为Element | null类型
}

/**
 * 处理后的剪辑数据接口
 * 
 * @interface
 * @property {string} sceneFormatted - 格式化后的场景编号（3位数字）
 * @property {string} shotFormatted - 格式化后的镜头编号（2位数字）
 * @property {string} takeFormatted - 格式化后的拍摄编号（2位数字）
 * @property {string} cameraId - 摄影机标识符（2位字母）
 * @property {string} rating - 拍摄评级（ok/kp/ng）
 */
interface ProcessedClipData {
  sceneFormatted: string;      
  shotFormatted: string;       
  takeFormatted: string;       
  cameraId: string;            
  rating: string;            
}

/**
 * 默认配置常量
 * 
 * @constant
 * @type {Required<XMLProcessConfig>}
 */
const DEFAULT_CONFIG = {
  width: 1920,                
  height: 1080,               
  format: '{scene}_{shot}_{take}{camera}{Rating}',   
  prefix: '',                 
  onProgress: () => {}      
} as const;

/**
 * 验证输入值有效性
 * 
 * @param {string} value - 需要验证的字符串
 * @returns {boolean} 是否有效
 * 
 * 有效性规则：
 * 1. 不能为空或纯空格
 * 2. 不能全为连字符
 * 3. 不能是" - "格式
 */
function isValidValue(value: string): boolean {
  value = value.trim();
  if (!value) return false;
  if (/^-+$/.test(value)) return false;
  if (/^\s*-\s*$/.test(value)) return false;
  return true;
}

/**
 * 格式化场景编号
 * 
 * @param {string} scene - 原始场景编号
 * @returns {string} 格式化后的场景编号（3位数字，大写）
 * 
 * @example
 * formatSceneNumber("A12") => "A012"
 */
function formatSceneNumber(scene: string): string {
  return scene.replace(/(\d+)/g, match => match.padStart(3, '0')).toUpperCase();
}

/**
 * 格式化镜头和拍摄编号
 * 
 * @param {string} value - 原始镜头拍摄字符串（格式：镜头-拍摄）
 * @returns {[string, string]} 格式化后的镜头和拍摄编号（各2位数字，小写）
 * 
 * @example
 * formatShotTake("3-5") => ["03", "05"]
 */
function formatShotTake(value: string): [string, string] {
  const [shot, take] = value.replace(/\s/g, '').split('-');
  return [
    shot.replace(/(\d+)/g, match => match.padStart(2, '0')).toLowerCase(),
    take.replace(/(\d+)/g, match => match.padStart(2, '0')).toLowerCase()
  ];
}

/**
 * 清理文件名中的无效字符
 * 
 * @param {string} name - 原始文件名
 * @returns {string} 清理后的文件名
 * 
 * 清理规则：
 * 1. 替换连续下划线为单个
 * 2. 去除末尾下划线
 */
function cleanupFileName(name: string): string {
  return name
    .replace(/_{2,}/g, '_')        
    .replace(/_+$/, '');       
}

/**
 * 从标签中提取拍摄评级
 * @param {Element | null} labelsElement - 标签元素
 * @returns {string} 评级标识（小写形式）
 * 
 * 提取规则：
 * - 如果包含"No Label"则不提取（返回空字符串）
 * - 特殊处理: "keep" 或 "kp" 统一返回 "kp"
 * - 其他任何内容都提取并转换为小写
 */
function getRatingFromLabels(labelsElement: Element | null): string {
  // 如果标签元素不存在，返回空字符串
  if (!labelsElement) return "";
  
  // 获取label元素的文本内容
  const labelElem = labelsElement.querySelector('label');
  const labelText = labelElem?.textContent?.trim() || "";
  
  // 调试输出
  console.log("label内容:", labelText);
  
  // 转换为小写进行比较
  const lowerLabelText = labelText.toLowerCase();
  
  // 如果包含"no label"，则不提取
  if (lowerLabelText.includes("no label")) {
    return ""; // 不识别
  }
  
  // 特殊处理: "keep" 或 "kp" 统一返回 "kp"
  if (lowerLabelText.includes("keep") || lowerLabelText.includes("kp")) {
    return "kp";
  }
  
  // 其他任何内容都提取并转换为小写
  return lowerLabelText;
}

/**
 * 获取摄影机标识符
 * 
 * @param {string} camerarollText - 摄影机卷号文本
 * @returns {string} 2位小写字母标识
 * 
 * @example
 * getCameraIdentifier("A001") => "a"
 * getCameraIdentifier("BCam002") => "bc"
 */
export function getCameraIdentifier(camerarollText: string): string {
  if (!camerarollText) return "";
  
  const match = camerarollText.match(/^[A-Za-z]+/);
  if (!match) return "";
  
  const letters = match[0];
  
  if (letters.length > 2) {
    return letters.slice(0, 2).toLowerCase();
  }
  
  return letters.toLowerCase();
}

/**
 * 从clip元素中提取必要子元素
 * 
 * @param {Element} clip - XML clip元素
 * @returns {ClipElements | null} 提取到的元素集合或null
 * 
 * 需要提取的元素包括：
 * - logginginfo
 * - scene
 * - shottake
 * - filmdata
 * - comments
 * - mastercomment2
 * - labels
 */
function extractClipElements(clip: Element): ClipElements | null {
  const logginginfo = clip.querySelector('logginginfo');
  const scene = logginginfo?.querySelector('scene');
  const shottake = logginginfo?.querySelector('shottake');
  const filmdata = clip.querySelector('filmdata');
  const comments = clip.querySelector('comments');
  const mastercomment2 = comments?.querySelector('mastercomment2');
  const labels = clip.querySelector('labels'); // 直接从clip中提取labels
  
  console.log("从clip中提取的labels元素:", labels);
  
  if (!logginginfo || !scene || !shottake || !filmdata || !comments || !mastercomment2) {
    return null;
  }
  
  return { logginginfo, scene, shottake, filmdata, comments, mastercomment2, labels };
}

/**
 * 处理剪辑数据
 * 
 * @param {ClipElements} elements - 提取到的元素集合
 * @returns {ProcessedClipData | null} 处理后的数据或null
 * 
 * 处理流程：
 * 1. 验证场景和镜头数据有效性
 * 2. 格式化场景、镜头、拍摄编号
 * 3. 提取摄影机标识
 * 4. 从labels元素提取拍摄评级
 */
function processClipData(elements: ClipElements): ProcessedClipData | null {
  const { scene, shottake, filmdata, labels } = elements;
  
  const sceneValue = scene.textContent || "";
  const shottakeValue = shottake.textContent || "";
  
  // 验证数据有效性
  if (!isValidValue(sceneValue) || !isValidValue(shottakeValue)) {
    return null;
  }
  
  // 验证镜头-拍摄格式
  const shotTakeParts = shottakeValue.split('-');
  if (shotTakeParts.length !== 2 || 
      !shotTakeParts[0].trim() || 
      !shotTakeParts[1].trim()) {
    return null;
  }
  
  // 格式化数据
  const sceneFormatted = formatSceneNumber(sceneValue);
  const [shotFormatted, takeFormatted] = formatShotTake(shottakeValue);
  
  // 提取摄影机标识
  const cameraroll = filmdata.querySelector('cameraroll');
  if (!cameraroll?.textContent) return null;
  
  const cameraId = getCameraIdentifier(cameraroll.textContent);
  
  // 提取评级信息 - 使用提取的labels元素
  const rating = getRatingFromLabels(labels);
  console.log("提取的评级:", rating);
  
  return {
    sceneFormatted,
    shotFormatted,
    takeFormatted,
    cameraId,
    rating
  };
}

/**
 * 生成新文件名
 * 
 * @param {ProcessedClipData} data - 处理后的剪辑数据
 * @param {Required<XMLProcessConfig>} config - 配置参数
 * @returns {string} 生成的文件名
 * 
 * 替换规则：
 * {scene} -> 场景编号
 * {shot} -> 镜头编号
 * {take} -> 拍摄编号
 * {camera} -> 摄影机标识
 * {Rating} -> 评级后缀（带下划线）
 */
function generateNewName(data: ProcessedClipData, config: Required<XMLProcessConfig>): string {
  console.log("生成文件名, 评级值:", data.rating);
  
  let newName = config.format
    .replace('{scene}', data.sceneFormatted)
    .replace('{shot}', data.shotFormatted)
    .replace('{take}', data.takeFormatted)
    .replace('{camera}', data.cameraId)
    .replace('{Rating}', data.rating ? `_${data.rating}` : '');
  
  console.log("替换{Rating}后:", newName);
    
  newName = config.prefix + cleanupFileName(newName);
  console.log("最终文件名:", newName);
  
  return newName;
}

/**
 * 处理标签元素，修正拼写错误
 * 
 * @param {Element | null} labelsElement - 标签元素
 * @returns {void} 
 */
function fixLabelsSpelling(labelsElement: Element | null): void {
  if (!labelsElement) return;
  
  // 查找label2元素
  const label2Elem = labelsElement.querySelector('label2');
  if (!label2Elem) return;
  
  // 获取label2的文本内容
  const label2Text = label2Elem.textContent || "";
  
  // 如果包含"Celurean"，修正为"Cerulean"
  if (label2Text.includes("Celurean")) {
    label2Elem.textContent = label2Text.replace("Celurean", "Cerulean");
    console.log("已修正拼写: Celurean -> Cerulean");
  }
}

/**
 * 更新相关XML元素
 * 
 * @param {Element} clip - 当前clip元素
 * @param {Document} xmlDoc - XML文档对象
 * @param {string} newName - 新文件名
 * 
 * 更新范围包括：
 * 1. 当前clip的name元素
 * 2. 关联sequence元素的name元素
 * 3. 关联clipitem的name元素
 * 4. 将clip的labels元素复制到sequence元素和其中的clipitem元素
 * 5. 修正label2中Celurean的拼写
 */
function updateRelatedElements(clip: Element, xmlDoc: Document, newName: string): void {
  const clipId = clip.getAttribute('id');
  if (!clipId) return;
  
  // 获取clip中的labels元素
  const labelsElem = clip.querySelector('labels');
  
  // 修正labels元素中的拼写错误
  fixLabelsSpelling(labelsElem);
  
  // 更新clip的name元素
  const nameElem = clip.querySelector('name');
  if (nameElem) {
    nameElem.textContent = newName;
  }
  
  // 查找关联的sequence元素
  const sequenceElem = xmlDoc.querySelector(`sequence[id="sequence_id_${clipId}"]`) ||
                      xmlDoc.querySelector(`sequence[id="sequence_${clipId}_ci"]`);
  
  if (sequenceElem) {
    // 更新sequence的name元素
    const sequenceName = sequenceElem.querySelector('name');
    if (sequenceName) {
      sequenceName.textContent = newName;
    }
    
    // 更新sequence中的clipitem相关元素
    updateClipItems(sequenceElem, newName, labelsElem);
    
    // 复制labels元素到sequence元素末尾
    if (labelsElem) {
      // 检查sequence是否已有labels元素
      const sequenceLabelsElem = sequenceElem.querySelector(':scope > labels');
      if (sequenceLabelsElem) {
        // 如果存在，则替换内容
        sequenceLabelsElem.innerHTML = labelsElem.innerHTML;
      } else {
        // 如果不存在，则复制并添加到sequence元素的末尾
        const clonedLabels = labelsElem.cloneNode(true);
        sequenceElem.appendChild(clonedLabels);
        console.log("已复制labels元素到sequence末尾:", clipId);
      }
    }
  }
}

/**
 * 更新sequence中的clipitem元素
 * 
 * @param {Element} sequenceElem - sequence元素
 * @param {string} newName - 新文件名
 * @param {Element | null} labelsElem - 标签元素
 */
function updateClipItems(sequenceElem: Element, newName: string, labelsElem: Element | null): void {
  // 更新所有video track中的clipitem
  const videoTrackClipitems = sequenceElem.querySelectorAll('video > track > clipitem');
  for (const clipitem of Array.from(videoTrackClipitems)) {
    const clipitemName = clipitem.querySelector('name');
    if (clipitemName) {
      clipitemName.textContent = newName;
    }
    
    // 复制labels元素到video clipitem
    copyLabelsToElement(clipitem, labelsElem);
  }
  
  // 更新所有audio track中的clipitem
  const audioTrackClipitems = sequenceElem.querySelectorAll('audio > track > clipitem');
  for (const clipitem of Array.from(audioTrackClipitems)) {
    // 复制labels元素到audio clipitem
    copyLabelsToElement(clipitem, labelsElem);
  }
}

/**
 * 复制labels元素到目标元素
 * 
 * @param {Element} targetElem - 目标元素
 * @param {Element | null} labelsElem - 标签元素
 */
function copyLabelsToElement(targetElem: Element, labelsElem: Element | null): void {
  if (!labelsElem) return;
  
  // 检查目标元素是否已有labels元素
  const targetLabelsElem = targetElem.querySelector('labels');
  if (targetLabelsElem) {
    // 如果存在，则替换内容
    targetLabelsElem.innerHTML = labelsElem.innerHTML;
  } else {
    // 如果不存在，则复制并添加到目标元素中
    const clonedLabels = labelsElem.cloneNode(true);
    targetElem.appendChild(clonedLabels);
  }
}

/**
 * 更新分辨率设置
 * 
 * @param {Document} xmlDoc - XML文档对象
 * @param {Required<XMLProcessConfig>} config - 配置参数
 * 
 * 更新所有width和height元素的文本内容
 */
function updateResolution(xmlDoc: Document, config: Required<XMLProcessConfig>): void {
  const widthElems = xmlDoc.getElementsByTagName('width');
  const heightElems = xmlDoc.getElementsByTagName('height');
  
  Array.from(widthElems).forEach(elem => {
    elem.textContent = config.width.toString();
  });
  
  Array.from(heightElems).forEach(elem => {
    elem.textContent = config.height.toString();
  });
}

/**
 * 更新DIT信息
 * 
 * @param {Document} xmlDoc - XML文档对象
 * 
 * 将所有"DIT: (null)"替换为"Generated by https://double-love.ahua.space"
 */
function updateDITInfo(xmlDoc: Document): void {
  const lognoteElems = xmlDoc.getElementsByTagName('lognote');
  Array.from(lognoteElems).forEach(elem => {
    if (elem.textContent === 'DIT: (null)') {
      elem.textContent = 'Generated by https://double-love.ahua.space';
    }
  });
}


/**
 * 处理路径URL函数
 * 
 * @param {Document} xmlDoc - XML文档对象
 * 
 * 使用正则表达式找到并替换pathurl元素中的序列帧文件名：
 * 1. 将匹配 \.\d+\.arx 的内容替换为 .arx
 * 2. 将匹配 \.\d+\.ari 的内容替换为 .ari
 * 3. 将匹配 _\d+\.dng 的内容替换为 .dng
 */
function processPathURLs(xmlDoc: Document): void {
  // 获取所有pathurl元素
  const pathurlElems = xmlDoc.getElementsByTagName('pathurl');
  
  // 遍历处理每个pathurl元素
  Array.from(pathurlElems).forEach(elem => {
    if (elem.textContent) {
      // 使用正则表达式替换 \.\d+\.arx 为 .arx
      let newValue = elem.textContent.replace(/\.\d+\.arx/g, '.arx');
      // 使用正则表达式替换 \.\d+\.ari 为 .ari
      newValue = newValue.replace(/\.\d+\.ari/g, '.ari');
      // 使用正则表达式替换 _\d+\.dng 为 .dng
      newValue = newValue.replace(/_\d+\.dng/g, '.dng');
      elem.textContent = newValue;
    }
  });
}



/**
 * 主处理函数
 * 
 * @param {File} file - 输入的XML文件
 * @param {XMLProcessConfig} [config] - 配置参数
 * @returns {Promise<string>} 处理后的XML字符串
 * 
 * 处理流程：
 * 1. 合并配置参数
 * 2. 解析XML文件
 * 3. 校验XML有效性
 * 4. 遍历处理所有clip元素
 * 5. 更新分辨率设置
 * 6. 更新DIT信息
 * 7. 序列化输出XML
 */
export async function processXML(file: File, config?: XMLProcessConfig): Promise<string> {
  // 合并配置
  const finalConfig = { ...DEFAULT_CONFIG, ...config };
  
  // 解析XML文件
  const text = await file.text();
  const parser = new DOMParser();
  const xmlDoc = parser.parseFromString(text, 'text/xml');
  
  // 验证XML有效性
  if (xmlDoc.getElementsByTagName('parsererror').length > 0) {
    throw new XMLProcessError(
      XMLProcessErrorType.INVALID_XML,
      '无效的XML文件'
    );
  }
  
  // 查找并修正所有独立的labels元素（不在clip或sequence内的）
  const allLabelsElems = xmlDoc.getElementsByTagName('labels');
  for (const labelsElem of Array.from(allLabelsElems)) {
    fixLabelsSpelling(labelsElem);
  }
  
  // 遍历处理所有clip元素
  const clips = xmlDoc.getElementsByTagName('clip');
  console.log("找到片段数量:", clips.length);
  
  for (const clip of Array.from(clips)) {
    try {
      console.log("处理片段:", clip.getAttribute('id'));
      
      // 提取必要元素
      const elements = extractClipElements(clip);
      if (!elements) {
        console.log("无法提取必要元素，跳过此片段");
        continue;
      }
      
      // 处理剪辑数据
      const processedData = processClipData(elements);
      if (!processedData) {
        console.log("处理数据失败，跳过此片段");
        continue;
      }
      
      // 生成新文件名
      const newName = generateNewName(processedData, finalConfig);
      
      // 更新相关元素
      updateRelatedElements(clip, xmlDoc, newName);
      
    } catch (error) {
      console.error('处理clip失败:', error);
      continue;
    }
  }
  
  // 更新分辨率设置
  updateResolution(xmlDoc, finalConfig);
  
  // 更新DIT信息
  updateDITInfo(xmlDoc);

  // 更新PathURL信息
  processPathURLs(xmlDoc);
  
  // 序列化输出XML
  const serializer = new XMLSerializer();
  return '<?xml version="1.0" encoding="UTF-8"?>\n' + 
         serializer.serializeToString(xmlDoc.documentElement);
}