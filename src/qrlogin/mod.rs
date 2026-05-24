// 各站点二维码登录
//   bilibili: GET /passport-login/web/qrcode/{generate,poll}(无加密)
//   netease : POST /weapi/login/qrcode/{unikey,client/login}(L14c 实现,每个 POST 都要 weapi 加密)

pub mod bilibili;
