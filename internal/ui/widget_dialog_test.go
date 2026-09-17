//go:build windows

package ui

import "testing"

func TestDialogStateTransitions(t *testing.T) {
	app := &WidgetApp{}

	app.showInfoDialog(0, "检查更新", "当前已是最新版本。")
	if !app.dialog.visible || app.dialog.confirm || app.dialog.title != "检查更新" {
		t.Fatalf("info dialog = %+v", app.dialog)
	}
	app.dismissDialog(0)
	if app.dialog.visible {
		t.Fatal("dialog should be hidden after dismiss")
	}

	confirmed := false
	app.showConfirmDialog(0, "发现新版本", "是否安装？", func() { confirmed = true })
	if !app.dialog.visible || !app.dialog.confirm || app.dialog.onConfirm == nil {
		t.Fatalf("confirm dialog = %+v", app.dialog)
	}
	app.confirmDialog(0)
	if !confirmed {
		t.Fatal("confirm callback was not invoked")
	}
	if app.dialog.visible || app.dialog.onConfirm != nil || app.dialog.confirm {
		t.Fatalf("dialog should be reset after confirm: %+v", app.dialog)
	}

	// 取消 / Esc 不得触发确认回调。
	called := false
	app.showConfirmDialog(0, "发现新版本", "是否安装？", func() { called = true })
	app.dismissDialog(0)
	if called {
		t.Fatal("dismiss must not invoke the confirm callback")
	}
}

// 对话框几何必须落在悬浮窗逻辑尺寸内，且按钮不重叠。
func TestDialogGeometryStaysInsideWidget(t *testing.T) {
	rects := map[string]RECT{
		"box":     dialogBoxRect(),
		"title":   dialogTitleRect(),
		"message": dialogMessageRect(),
		"ok":      dialogOKRect(),
		"cancel":  dialogCancelRect(),
	}
	for name, rect := range rects {
		if rect.Left < 0 || rect.Top < 0 || rect.Right > widgetWidth || rect.Bottom > widgetHeight {
			t.Fatalf("%s out of widget bounds: %+v", name, rect)
		}
		if rect.Right <= rect.Left || rect.Bottom <= rect.Top {
			t.Fatalf("%s has non-positive size: %+v", name, rect)
		}
	}
	ok := dialogOKRect()
	cancel := dialogCancelRect()
	if ok.Left < cancel.Right {
		t.Fatalf("ok/cancel overlap: ok=%+v cancel=%+v", ok, cancel)
	}
	box := dialogBoxRect()
	if dialogTitleRect().Top < box.Top || dialogOKRect().Bottom > box.Bottom || dialogMessageRect().Bottom > dialogOKRect().Top {
		t.Fatalf("dialog content outside card: box=%+v title=%+v message=%+v ok=%+v",
			box, dialogTitleRect(), dialogMessageRect(), ok)
	}
}
