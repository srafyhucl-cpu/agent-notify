//go:build windows

package ui

// ThemePalette defines all colors used across the Fluent UI.
type ThemePalette struct {
	Name                 string
	IsDark               bool
	Background           uint32
	CardBg               uint32
	CardBgHover          uint32
	CardBorder           uint32
	CardBorderHover      uint32
	TextPrimary          uint32
	TextSecondary        uint32
	TextMuted            uint32
	ButtonBg             uint32
	ButtonBgHover        uint32
	ButtonBorder         uint32
	ButtonBorderHover    uint32
	InputBg              uint32
	InputBorder          uint32
	Divider              uint32
	WindowBtnHover       uint32
	WindowBtnDangerHover uint32
	KnobColor            uint32
	SwitchTrackOff       uint32
	SwitchTrackOn        uint32
	AccentSuccess        uint32
	AccentWarning        uint32
	AccentDanger         uint32
	BadgeBg              uint32
	DropdownBg           uint32
	DropdownBorder       uint32
	DropdownHover        uint32
	DiagBoxBg            uint32
	DiagBoxBorder        uint32
}

var (
	ThemeDark = ThemePalette{
		Name:                 "dark",
		IsDark:               true,
		Background:           RGB(15, 19, 23),
		CardBg:               RGB(22, 28, 35),
		CardBgHover:          RGB(28, 36, 45),
		CardBorder:           RGB(40, 50, 60),
		CardBorderHover:      RGB(58, 72, 88),
		TextPrimary:          RGB(242, 246, 248),
		TextSecondary:        RGB(165, 178, 190),
		TextMuted:            RGB(110, 122, 135),
		ButtonBg:             RGB(23, 29, 36),
		ButtonBgHover:        RGB(34, 44, 55),
		ButtonBorder:         RGB(40, 50, 60),
		ButtonBorderHover:    RGB(62, 78, 95),
		InputBg:              RGB(27, 34, 43),
		InputBorder:          RGB(48, 60, 72),
		Divider:              RGB(28, 36, 46),
		WindowBtnHover:       RGB(34, 41, 48),
		WindowBtnDangerHover: RGB(122, 42, 49),
		KnobColor:            RGB(244, 249, 248),
		SwitchTrackOff:       RGB(62, 72, 82),
		SwitchTrackOn:        RGB(39, 143, 113),
		AccentSuccess:        RGB(56, 194, 151),
		AccentWarning:        RGB(224, 165, 70),
		AccentDanger:         RGB(224, 104, 104),
		BadgeBg:              RGB(31, 40, 48),
		DropdownBg:           RGB(24, 30, 38),
		DropdownBorder:       RGB(45, 56, 68),
		DropdownHover:        RGB(36, 46, 58),
		DiagBoxBg:            RGB(18, 23, 28),
		DiagBoxBorder:        RGB(30, 38, 46),
	}

	ThemeLight = ThemePalette{
		Name:                 "light",
		IsDark:               false,
		Background:           RGB(243, 245, 247),
		CardBg:               RGB(255, 255, 255),
		CardBgHover:          RGB(246, 248, 250),
		CardBorder:           RGB(222, 228, 235),
		CardBorderHover:      RGB(198, 206, 216),
		TextPrimary:          RGB(28, 37, 48),
		TextSecondary:        RGB(82, 95, 110),
		TextMuted:            RGB(138, 149, 163),
		ButtonBg:             RGB(255, 255, 255),
		ButtonBgHover:        RGB(240, 244, 248),
		ButtonBorder:         RGB(222, 228, 235),
		ButtonBorderHover:    RGB(198, 206, 216),
		InputBg:              RGB(255, 255, 255),
		InputBorder:          RGB(212, 218, 226),
		Divider:              RGB(230, 234, 240),
		WindowBtnHover:       RGB(230, 235, 242),
		WindowBtnDangerHover: RGB(248, 215, 218),
		KnobColor:            RGB(255, 255, 255),
		SwitchTrackOff:       RGB(198, 205, 214),
		SwitchTrackOn:        RGB(34, 150, 115),
		AccentSuccess:        RGB(28, 142, 107),
		AccentWarning:        RGB(210, 138, 30),
		AccentDanger:         RGB(214, 65, 65),
		BadgeBg:              RGB(238, 242, 246),
		DropdownBg:           RGB(255, 255, 255),
		DropdownBorder:       RGB(218, 224, 232),
		DropdownHover:        RGB(242, 245, 250),
		DiagBoxBg:            RGB(246, 248, 250),
		DiagBoxBorder:        RGB(228, 232, 238),
	}
)

func GetTheme(themeName string) ThemePalette {
	if themeName == "light" {
		return ThemeLight
	}
	return ThemeDark
}
