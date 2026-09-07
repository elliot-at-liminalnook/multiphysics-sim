"""Physical video pixels must not inherit the display's Retina scale."""
import pytest
from PySide6.QtGui import QImage, QColor
from robocad.ui.motion_video import video_canvas


@pytest.mark.parametrize('dpi', [1., 1.5, 2.])
def test_portrait_video_content_fills_height_and_is_centered(dpi):
    source=QImage(100,200,QImage.Format_RGB888)
    source.fill(QColor('red'));source.setDevicePixelRatio(dpi)
    frame=video_canvas(source,320,180)
    assert frame.size().width()==320 and frame.size().height()==180
    assert frame.devicePixelRatio()==1.
    # A 90 x 180 portrait image should occupy x=115..204, all 180 rows.
    assert frame.pixelColor(115,0)==QColor('red')
    assert frame.pixelColor(204,179)==QColor('red')
    assert frame.pixelColor(114,90)==QColor('#202429')
    assert frame.pixelColor(205,90)==QColor('#202429')
    assert source.devicePixelRatio()==dpi


@pytest.mark.parametrize('dpi', [1.,2.])
def test_landscape_video_fills_canvas_without_a_second_dpi_scale(dpi):
    source=QImage(640,360,QImage.Format_RGB888)
    source.fill(QColor('green'));source.setDevicePixelRatio(dpi)
    frame=video_canvas(source,320,180)
    for x,y in ((0,0),(319,0),(0,179),(319,179)):
        assert frame.pixelColor(x,y)==QColor('green')
