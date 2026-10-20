# 图片路径六态

## 一、中文 + 空格 + 括号

![中文 图 (A)](media/图 中文 (A).png)

## 二、带 title 语法

![标题图](media/fig-normal.png "插入标题不应进入正文")

## 三、相对路径

![相对路径图](media/fig-normal.png)

## 四、URL 图片（未加 --download-images 应报缺图）

![远程图](https://example.com/remote-not-downloaded.png)

## 五、缺图（文件不存在）

![缺失图](media/not-exist-anywhere.png)

## 六、缺图之后的正文

缺图不影响后续正文渲染。
