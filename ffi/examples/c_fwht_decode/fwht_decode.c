#include <errno.h>
#include <fcntl.h>
#include <linux/dma-heap.h>
#include <linux/videodev2.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

#include "v4l2r.h"

struct allocated_dmabufs {
  size_t nb_buffers;
  struct v4l2r_video_frame buffers[16];
};

// Helper function to allocate DMABUF memory from a V4L2 device.
static struct allocated_dmabufs
allocate_dmabufs(const struct v4l2_format *format,
                 size_t nb_buffers) {
  struct allocated_dmabufs dmabufs;
  int dma_device;
  int ret;
  int i;

  memset(&dmabufs, 0, sizeof(dmabufs));

  dma_device = open("/dev/dma_heap/system", O_RDWR | O_CLOEXEC);
  if (dma_device < 0) {
    perror("error opening DMA heap device");
    goto end;
  }

  for (i = 0; i < nb_buffers; i++) {
    struct dma_heap_allocation_data allocation_data;

    // TODO support multiple planes? This is not strictly needed for
    // FWHT/RGB3...
    memset(&allocation_data, 0, sizeof(allocation_data));
    allocation_data.len = format->fmt.pix_mp.plane_fmt[0].sizeimage;
    allocation_data.fd_flags = O_CLOEXEC | O_RDWR;
    ret = ioctl(dma_device, DMA_HEAP_IOCTL_ALLOC, &allocation_data);
    if (ret < 0) {
      perror("error while allocating DMA memory");
      goto close_dev;
    }

    dmabufs.buffers[i].id = i;
    dmabufs.buffers[i].num_planes = 1;
    dmabufs.buffers[i].planes[0] = allocation_data.fd;
  }

  dmabufs.nb_buffers = nb_buffers;

close_dev:
  close(dma_device);

end:
  return dmabufs;
}

static const char *input_file_path = "sample.fwht";
static size_t input_frame_sizes[] = {
    39504, 5822, 42410, 5822, 42106, 5822, 41802, 7646, 40606, 8468,
    42640, 6644, 42928, 6644, 42624, 8468, 40540, 8846, 43002, 8846,
};

static const struct v4l2r_video_frame_provider *capture_provider = NULL;
static bool drain_completed = false;

const char *device_path = "/dev/video1";

static void on_input_done(void *ptr, const struct v4l2_buffer *buffer) {
  printf("C: input done: %p %d\n", ptr, buffer->index);
}

static void
on_frame_decoded(void *ptr,
                 const struct v4l2r_decoder_frame_decoded_event *event) {
  printf("C: frame decoded: %p %d %d, timestamp %ld\n", ptr,
         event->buffer->index, event->buffer->m.planes[0].bytesused,
         event->buffer->timestamp.tv_sec);

  printf("C: recycling frame %d\n", event->frame.id);
  v4l2r_video_frame_provider_queue_frame(capture_provider, event->frame);
}

static struct allocated_dmabufs dmabufs;

static void
on_format_change(void *ptr,
                 const struct v4l2r_decoder_format_changed_event *event) {
  const struct v4l2_format *format = event->new_format;
  const struct v4l2_rect *visible_rect = &event->visible_rect;
  int i;

  printf("C: new CAPTURE format: %p %x, %dx%d, %d frames, visible rect: "
         "(%d,%d),%ux%u \n",
         ptr, format->fmt.pix_mp.pixelformat, format->fmt.pix_mp.width,
         format->fmt.pix_mp.height, event->min_num_frames, visible_rect->left,
         visible_rect->top, visible_rect->width, visible_rect->height);

  if (capture_provider)
    v4l2r_video_frame_provider_drop(capture_provider);
  capture_provider = event->new_provider;

  dmabufs = allocate_dmabufs(format, event->min_num_frames);
  printf("C: Got %zu CAPTURE frames\n", dmabufs.nb_buffers);
  for (i = 0; i < dmabufs.nb_buffers; i++)
    v4l2r_video_frame_provider_queue_frame(capture_provider,
                                           dmabufs.buffers[i]);
}

void on_event(void *ptr, struct v4l2r_decoder_event *event) {
  switch (event->tag) {
  case FrameDecoded:
    on_frame_decoded(ptr, &event->frame_decoded);
    break;
  case FormatChanged:
    on_format_change(ptr, &event->format_changed);
    break;
  case EndOfStream:
    printf("Drain completed!\n");
    drain_completed = true;
    break;
  }
}

int main() {
  struct allocated_dmabufs dmabufs;
  struct v4l2_format output_format;
  size_t output_buffer_size;
  int output_dmabuf;
  int i;
  int ret;

  FILE *input_file = fopen(input_file_path, "r");
  if (!input_file) {
    perror("Cannot open input file");
    return 1;
  }

  v4l2r_init();

  struct v4l2r_decoder *decoder =
      v4l2r_decoder_new(device_path, V4L2_PIX_FMT_FWHT, 1, 0, 0, on_input_done,
                        on_event, (void *)0xdeadbeef);
  printf("C: Got decoder: %p\n", decoder);

  ret = v4l2r_decoder_get_input_format(decoder, &output_format);
  if (ret < 0)
    return ret;
  printf("reported output format: %x %d\n",
         output_format.fmt.pix_mp.pixelformat,
         output_format.fmt.pix_mp.plane_fmt[0].sizeimage);
  dmabufs = allocate_dmabufs(&output_format, 1);
  if (dmabufs.nb_buffers < 1) {
    return -1;
  }
  output_dmabuf = dmabufs.buffers[0].planes[0];
  output_buffer_size = output_format.fmt.pix_mp.plane_fmt[0].sizeimage;
  printf("C: Got DMABUF: %lu %d %zu\n", dmabufs.buffers[0].num_planes,
         dmabufs.buffers[0].planes[0], output_buffer_size);

  for (i = 0; i < 20; i++) {
    size_t frame_bytes_used = input_frame_sizes[i];
    void *mapping = mmap(NULL, output_buffer_size, PROT_READ | PROT_WRITE,
                         MAP_SHARED, output_dmabuf, 0);
    if (!mapping) {
      perror("Error while mapping");
      return 1;
    }
    if (fread(mapping, frame_bytes_used, 1, input_file) != 1) {
      perror("Error reading file");
      return 1;
    }
    munmap(mapping, output_buffer_size);

    ret = v4l2r_decoder_decode(decoder, i, output_dmabuf, frame_bytes_used);
    if (ret < 0)
      return 1;
  }

  v4l2r_decoder_drain(decoder, false);
  while (!drain_completed)
    usleep(10000);

  v4l2r_decoder_destroy(decoder);
  if (capture_provider)
    v4l2r_video_frame_provider_drop(capture_provider);
  printf("C: all done\n");

  close(output_dmabuf);
  fclose(input_file);

  return 0;
}
