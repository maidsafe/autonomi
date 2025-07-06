// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use itertools::Itertools;
use ratatui::{prelude::*, widgets::WidgetRef};

/// A hyperlink widget that renders a hyperlink in the terminal using [OSC 8].
///
/// [OSC 8]: https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda
pub struct Hyperlink<'content> {
    text: Text<'content>,
    url: String,
}

impl<'content> Hyperlink<'content> {
    pub fn new(text: impl Into<Text<'content>>, url: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            url: url.into(),
        }
    }
}

impl WidgetRef for Hyperlink<'_> {
    fn render_ref(&self, area: Rect, buffer: &mut Buffer) {
        self.text.render_ref(area, buffer);

        // this is a hacky workaround for https://github.com/ratatui-org/ratatui/issues/902, a bug
        // in the terminal code that incorrectly calculates the width of ANSI escape sequences. It
        // works by rendering the hyperlink as a series of 2-character chunks, which is the
        // calculated width of the hyperlink text.
        for (i, two_chars) in self
            .text
            .to_string()
            .chars()
            .chunks(2)
            .into_iter()
            .enumerate()
        {
            let text = two_chars.collect::<String>();
            let hyperlink = format!("\x1B]8;;{}\x07{}\x1B]8;;\x07", self.url, text);
            buffer
                .cell_mut(Position::new(area.x + i as u16 * 2, area.y))
                .map(|cell| cell.set_symbol(hyperlink.as_str()));
        }
    }
}
