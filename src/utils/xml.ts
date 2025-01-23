export async function processXML(file: File, config?: {
  width?: number;
  height?: number;
  format?: string;
  prefix?: string;
}): Promise<string> {
  const text = await file.text();
  const parser = new DOMParser();
  const xmlDoc = parser.parseFromString(text, 'text/xml');

  if (xmlDoc.getElementsByTagName('parsererror').length > 0) {
    throw new Error('无效的XML文件');
  }

  // 处理所有clip元素
  const clips = xmlDoc.getElementsByTagName('clip');
  for (const clip of Array.from(clips)) {
    const logginginfo = clip.querySelector('logginginfo');
    if (!logginginfo) continue;

    const scene = logginginfo.querySelector('scene');
    const shottake = logginginfo.querySelector('shottake');
    
    if (!scene || !shottake) continue;

    let sceneValue = scene.textContent || "";
    sceneValue = sceneValue.replace(/(\d+)/g, (match) => match.padStart(3, '0'));
    sceneValue = sceneValue.toUpperCase();

    const [shotValue, takeValue] = shottake.textContent?.replace(/\s/g, '').split('-') || ['', ''];
    const shotFormatted = shotValue.replace(/(\d+)/g, (match) => 
      match.padStart(2, '0')).toLowerCase();
    const takeFormatted = takeValue.replace(/(\d+)/g, (match) => 
      match.padStart(2, '0')).toLowerCase();

    const filmdata = clip.querySelector('filmdata');
    if (!filmdata) continue;

    const cameraroll = filmdata.querySelector('cameraroll');
    if (!cameraroll || !cameraroll.textContent) continue;

    const cameraIdentifier = getCameraIdentifier(cameraroll.textContent);

    const comments = clip.querySelector('comments');
    if (!comments) continue;

    const mastercomment2 = comments.querySelector('mastercomment2');
    if (!mastercomment2) continue;

    let mastercomment2Value = mastercomment2.textContent?.trim() || "";
    if (mastercomment2Value.endsWith(",")) {
      mastercomment2Value = mastercomment2Value.slice(0, -1);
    }

    let ratingvalue = "";
    if (mastercomment2Value.includes("Circle")) {
      ratingvalue = "ok";
    } else if (mastercomment2Value.includes("KEEP")) {
      ratingvalue = "kp";
    } else if (mastercomment2Value.includes("NG")) {
      ratingvalue = "ng";
    }

    // 默认格式
    const defaultFormat = '{scene}_{shot}_{take}{camera}{Rating}';
    const format = config?.format || defaultFormat;
    
    // 替换占位符
    let newNameValue = format
      .replace('{scene}', sceneValue)
      .replace('{shot}', shotFormatted)
      .replace('{take}', takeFormatted)
      .replace('{camera}', cameraIdentifier);

    if (ratingvalue) {
      newNameValue = (config?.prefix || '') + newNameValue.replace('{Rating}', `_${ratingvalue}`);
    } else {
      newNameValue = (config?.prefix || '') + newNameValue.replace('{Rating}', '');
      // 移除末尾的下划线
      if (newNameValue.endsWith('_')) {
        newNameValue = newNameValue.slice(0, -1);
      }
    }
    // 替换可能出现的双下划线
    newNameValue = newNameValue.replace(/_{2,}/g, '_');

    const nameElem = clip.querySelector('name');
    if (nameElem) {
      nameElem.textContent = newNameValue;
    }

    const clipId = clip.getAttribute('id');
    if (clipId) {
      const sequenceElem = xmlDoc.querySelector(`sequence[id="sequence_id_${clipId}"]`) ||
                          xmlDoc.querySelector(`sequence[id="sequence_${clipId}_ci"]`);
      
      if (sequenceElem) {
        const sequenceName = sequenceElem.querySelector('name');
        if (sequenceName) {
          sequenceName.textContent = newNameValue;
        }

        const nameElements = sequenceElem.getElementsByTagName('name');
        if (nameElements.length >= 3) {
          nameElements[2].textContent = newNameValue;
        }

        const clipitem = sequenceElem.querySelector(`clipitem[id="sequence_${clipId}_ci"]`);
        if (clipitem) {
          const clipitemName = clipitem.querySelector('name');
          if (clipitemName) {
            clipitemName.textContent = newNameValue;
          }
        }
      }
    }
  }

  // 更新分辨率
  const widthElems = xmlDoc.getElementsByTagName('width');
  const heightElems = xmlDoc.getElementsByTagName('height');
  
  const targetWidth = config?.width || 1920;
  const targetHeight = config?.height || 1080;
  
  Array.from(widthElems).forEach(elem => {
    elem.textContent = targetWidth.toString();
  });
  
  Array.from(heightElems).forEach(elem => {
    elem.textContent = targetHeight.toString();
  });

  // 更新DIT信息
  const lognoteElems = xmlDoc.getElementsByTagName('lognote');
  Array.from(lognoteElems).forEach(elem => {
    if (elem.textContent === 'DIT: (null)') {
      elem.textContent = 'DIT: 哆啦Ahua 🌱';
    }
  });

  const serializer = new XMLSerializer();
  return '<?xml version="1.0" encoding="UTF-8"?>\n' + 
         serializer.serializeToString(xmlDoc.documentElement);
}

export function getCameraIdentifier(camerarollText: string): string {
  if (!camerarollText) return "";
  
  const firstPart = camerarollText.includes("_") 
    ? camerarollText.split("_")[0] 
    : camerarollText;
    
  if (firstPart.length >= 2 && 
      firstPart[0].match(/[a-zA-Z]/) && 
      firstPart[1].match(/[a-zA-Z]/)) {
    return firstPart.slice(0, 2).toLowerCase();
  }
  return "";
}
