// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Clear},
    Frame,
};

pub const GHOST_WHITE: Color = Color::Indexed(15);
pub const COOL_GREY: Color = Color::Indexed(246);
pub const LIGHT_PERIWINKLE: Color = Color::Indexed(252);
pub const VERY_LIGHT_AZURE: Color = Color::Indexed(75);
pub const EUCALYPTUS: Color = Color::Indexed(115);
pub const SIZZLING_RED: Color = Color::Indexed(197);
pub const SPACE_CADET: Color = Color::Indexed(17);
pub const DARK_GUNMETAL: Color = Color::Indexed(235); // 266 is incorrect
pub const INDIGO: Color = Color::Indexed(24);
pub const VIVID_SKY_BLUE: Color = Color::Indexed(45);
pub const RED: Color = Color::Indexed(196);

// Clears the area and sets the background color
pub fn clear_area(f: &mut Frame<'_>, area: Rect) {
    f.render_widget(Clear, area);
    f.render_widget(Block::new().style(Style::new().bg(DARK_GUNMETAL)), area);
}
