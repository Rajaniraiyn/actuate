// Manual native drag diagnostic. See docs/macos-drag-validation.md.
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
@interface Probe : NSView <NSDraggingSource,NSDraggingDestination>
@property BOOL dragging;
@property BOOL sourcePressed;
@end
@implementation Probe
-(void)sliderChanged:(NSSlider*)s {printf("SLIDER=%.3f\n",s.doubleValue);fflush(stdout);}
-(BOOL)acceptsFirstMouse:(NSEvent*)e {return YES;}
-(void)drawRect:(NSRect)r {[[NSColor windowBackgroundColor] setFill];NSRectFill(self.bounds);[[NSColor systemBlueColor]setFill];NSRectFill(NSMakeRect(30,70,100,100));[[NSColor systemGreenColor]setFill];NSRectFill(NSMakeRect(330,70,130,100));[@"Drag blue square into green square" drawAtPoint:NSMakePoint(25,220) withAttributes:nil];}
-(void)log:(NSEvent*)e {printf("event=%lu point=%.1f,%.1f timestamp=%.9f delta=%.1f,%.1f buttons=%lu pressure=%.2f\n",(unsigned long)e.type,e.locationInWindow.x,e.locationInWindow.y,e.timestamp,e.deltaX,e.deltaY,(unsigned long)NSEvent.pressedMouseButtons,e.pressure);fflush(stdout);}
-(void)mouseDown:(NSEvent*)e {self.dragging=NO;self.sourcePressed=NSPointInRect(e.locationInWindow,NSMakeRect(30,70,100,100));[self log:e];}
-(void)mouseUp:(NSEvent*)e {[self log:e];self.dragging=NO;self.sourcePressed=NO;}
-(void)mouseDragged:(NSEvent*)e {[self log:e];if(self.dragging || !self.sourcePressed)return;self.dragging=YES;NSPasteboardItem*p=[NSPasteboardItem new];[p setString:@"unimation-drag-probe" forType:NSPasteboardTypeString];NSDraggingItem*i=[[NSDraggingItem alloc]initWithPasteboardWriter:p];NSImage*im=[[NSImage alloc]initWithSize:NSMakeSize(40,40)];[im lockFocus];[[NSColor systemBlueColor]setFill];NSRectFill(NSMakeRect(0,0,40,40));[im unlockFocus];[i setDraggingFrame:NSMakeRect(e.locationInWindow.x,e.locationInWindow.y,40,40) contents:im];[self beginDraggingSessionWithItems:@[i] event:e source:self];printf("begin_returned\n");fflush(stdout);}
-(NSDragOperation)draggingSession:(NSDraggingSession*)s sourceOperationMaskForDraggingContext:(NSDraggingContext)c {return NSDragOperationCopy;}
-(void)draggingSession:(NSDraggingSession*)s movedToPoint:(NSPoint)p {printf("drag_moved=%.1f,%.1f\n",p.x,p.y);fflush(stdout);}
-(void)draggingSession:(NSDraggingSession*)s endedAtPoint:(NSPoint)p operation:(NSDragOperation)o {printf("drag_end=%.1f,%.1f operation=%lu\n",p.x,p.y,(unsigned long)o);self.dragging=NO;fflush(stdout);}
-(NSDragOperation)draggingEntered:(id<NSDraggingInfo>)i {printf("entered\n");fflush(stdout);return NSDragOperationCopy;}
-(BOOL)performDragOperation:(id<NSDraggingInfo>)i {NSPoint p=i.draggingLocation;printf("DROP=%.1f,%.1f\n",p.x,p.y);fflush(stdout);return YES;}
@end
int main(){@autoreleasepool{NSApplication*a=NSApplication.sharedApplication;[a setActivationPolicy:NSApplicationActivationPolicyAccessory];NSWindow*w=[[NSWindow alloc]initWithContentRect:NSMakeRect(2700,350,500,280) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable backing:NSBackingStoreBuffered defer:NO];w.title=@"Unimation drag probe";Probe*v=[[Probe alloc]initWithFrame:NSMakeRect(0,0,500,280)];[v registerForDraggedTypes:@[NSPasteboardTypeString]];NSSlider*slider=[[NSSlider alloc]initWithFrame:NSMakeRect(30,20,420,24)];slider.minValue=0;slider.maxValue=100;slider.doubleValue=0;slider.continuous=YES;slider.target=v;slider.action=@selector(sliderChanged:);[v addSubview:slider];w.contentView=v;[w orderFrontRegardless];printf("pid=%d window=%ld\n",getpid(),w.windowNumber);fflush(stdout);[a run];}}
