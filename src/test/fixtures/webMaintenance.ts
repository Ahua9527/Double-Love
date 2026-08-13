export const SYNTHETIC_CLIP_ID = 'SYNTHETIC_CLIP_A001'

export const SYNTHETIC_PREMIERE_XML = `<?xml version="1.0" encoding="UTF-8"?>
<project>
  <media>
    <clip id="${SYNTHETIC_CLIP_ID}">
      <name>合成测试片段</name>
      <logginginfo>
        <scene>26</scene>
        <shottake>4-1</shottake>
      </logginginfo>
      <filmdata>
        <cameraroll>A001</cameraroll>
      </filmdata>
      <comments>
        <mastercomment2>合成测试备注</mastercomment2>
      </comments>
      <labels>
        <label>KEEP</label>
      </labels>
    </clip>
  </media>
  <sequence id="sequence_id_${SYNTHETIC_CLIP_ID}">
    <name>合成测试序列</name>
    <width>1280</width>
    <height>720</height>
    <video>
      <track>
        <clipitem>
          <name>合成测试片段</name>
        </clipitem>
      </track>
    </video>
  </sequence>
  <lognote>DIT: (null)</lognote>
</project>`
