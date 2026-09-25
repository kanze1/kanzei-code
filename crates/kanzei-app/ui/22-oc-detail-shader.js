// Keep source ink outside the video codec. Only detail residuals are added to
// the clean face; no skin-coloured rectangle is pasted over the moving frame.
export const OC_DETAIL_FRAGMENT = `
precision highp float;
varying vec2 vUv;
uniform sampler2D uVideoA;
uniform sampler2D uVideoB;
uniform sampler2D uMouthArt;
uniform sampler2D uClosedMouthArt;
uniform sampler2D uTattooArt;
uniform vec4 uMouthA;
uniform vec4 uMouthB;
uniform vec4 uTattooA;
uniform vec4 uTattooB;
uniform vec2 uMouthReference;
uniform vec2 uTattooReference;
uniform vec2 uTattooTextureSize;
uniform float uTattooScale;
uniform vec2 uSize;
uniform vec2 uPacking;
uniform float uBlend;
uniform float uMouth;

vec2 detailLocal(vec4 track){
  vec2 p=(vUv-track.xy)*uSize;
  float c=cos(track.w),s=sin(track.w);
  return vec2(c*p.x+s*p.y,-s*p.x+c*p.y)/max(track.z,.8);
}
vec3 lipInk(sampler2D art,vec2 local){
  float support=1.0-smoothstep(.8,1.0,length(local/vec2(.024,.010)));
  if(support<=0.0)return vec3(0.0);
  vec2 top=uMouthReference+vec2(local.x,-.013);
  vec2 bottom=uMouthReference+vec2(local.x,.013);
  vec3 skin=mix(texture2D(art,top).rgb,texture2D(art,bottom).rgb,clamp(.5+local.y/.026,0.0,1.0));
  return (texture2D(art,uMouthReference+local).rgb-skin)*support;
}
vec4 character(sampler2D art,vec4 mouthTrack,vec4 tattooTrack,float isPacked){
  vec2 uv=vec2(vUv.x,vUv.y*mix(1.0,.5,isPacked));
  vec4 colour=texture2D(art,uv);
  float alpha=isPacked>.5?texture2D(art,vec2(vUv.x,.5+vUv.y*.5)).r:colour.a;
  if(alpha<.004)return vec4(0.0);
  vec2 local=detailLocal(mouthTrack)/uSize;
  float mouthRegion=1.0-smoothstep(.72,1.0,length(local/vec2(.028,.014)));
  if(mouthRegion>0.0){
    vec2 openLocal=vec2(local.x,(local.y+.0015)/mix(.08,1.0,uMouth)-.0035);
    vec3 ink=mix(lipInk(uClosedMouthArt,local),lipInk(uMouthArt,openLocal),smoothstep(.10,.16,uMouth));
    colour.rgb=clamp(colour.rgb+ink*mouthRegion,0.0,1.0);
  }
  vec2 tattooLocal=detailLocal(tattooTrack)/uTattooScale;
  if(abs(tattooLocal.x)<25.0&&tattooLocal.y> -92.0&&tattooLocal.y<110.0){
    vec2 source=uTattooReference+tattooLocal;
    vec3 pigment=texture2D(uTattooArt,source/uTattooTextureSize).rgb;
    vec3 left=texture2D(uTattooArt,vec2(uTattooReference.x-40.0,source.y)/uTattooTextureSize).rgb;
    vec3 right=texture2D(uTattooArt,vec2(uTattooReference.x+40.0,source.y)/uTattooTextureSize).rgb;
    vec3 skin=mix(left,right,clamp(.5+tattooLocal.x/80.0,0.0,1.0));
    float coverage=clamp(1.0-(pigment.r-pigment.b)/max(.12,skin.r-skin.b),0.0,1.0);
    coverage=smoothstep(.08,.92,coverage);
    vec3 ink=clamp((pigment-(1.0-coverage)*skin)/max(coverage,.01),0.0,1.0);
    colour.rgb=mix(colour.rgb,ink,coverage);
  }
  return vec4(colour.rgb*alpha,alpha);
}
void main(){
  if(uBlend>=.999)gl_FragColor=character(uVideoB,uMouthB,uTattooB,uPacking.y);
  else gl_FragColor=mix(character(uVideoA,uMouthA,uTattooA,uPacking.x),character(uVideoB,uMouthB,uTattooB,uPacking.y),uBlend);
}`;
