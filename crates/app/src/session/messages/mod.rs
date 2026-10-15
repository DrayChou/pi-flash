//! 消息域（032）：模型层 / 图片 / 卡片 / 行轮渲染 / TextView 包装五件套。
//! 子模块条目经 glob re-export 保持 `messages::X` 旧路径全部可用。

mod cards;
mod images;
mod model;
mod textview;
mod turn;

pub(crate) use cards::*;
pub(crate) use images::*;
pub(crate) use model::*;
pub(crate) use textview::*;
pub(crate) use turn::*;
